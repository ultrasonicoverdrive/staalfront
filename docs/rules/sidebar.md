# The sidebar

Work package WP11.1 (PLAN.md phase 11), built before M1 by the orchestrator's decision of 01-11-2026: the
debug keys of WP6.5 (U, I, O, G) are not a way to play. Presentation only: no simulation code changed, no new
command and no checksum moved. Code: `crates/sf_game/src/sidebar.rs` (what the tabs show and what a click does),
`sidebar_layout.rs` (the geometry, the hit test, scrolling), `sidebar_view.rs` (drawing, reading the presses),
`hud.rs` (the few lines that call it) and `runner.rs` (turning an action into a command). Data:
`assets/config/sidebar.ron` (`sf_data::sidebar_look`) and the `build_picture` of `unit_defs.ron`. Tests are named
below.

Rules in our own words (clean-room, B5); no Red Alert art or layout was used (B11).

## What it shows

A column on the right edge of the window, 200 logical pixels wide, scaled with the UI scale, top to bottom:

1. **Credits** ("Credits 1000"), in the warning colour when the credits are few or a build waits for them: the
   readout and colours of `money_look.ron` (WP7.4), moved into the column. Without a sidebar (the crowd scene) the old
   readout in the top right corner stays. In a world without money rules the block is empty.
2. **The power bar.** Power does not exist yet (WP6.4). `sidebar::power_reading` is the hook: it returns `None`, and
   the bar then draws a neutral frame saying "Power: no power rules". WP6.4 returns a `PowerReading { made, used }` and
   the bar fills with what is used against the larger of made and used, in the low colour when more is used than made
   (`power_fill`, tested; the drawing of both states is `the_credits_the_power_bar_and_the_tabs_are_drawn`).
3. **Four tabs**: Structures, Infantry, Vehicles, Vessels, two by two. The tab shown is lit; a tab with nothing to
   build has dimmed text but can still be opened.
4. **A grid of items**, two per row, as many rows as fit. The wheel over the column scrolls it a row at a time
   (the wheel there is not a camera zoom).
5. **The minimap** stays where it was, bottom right, 200 px wide, under the column (`bottom_reserve_px` is the room
   it takes). It is not part of the sidebar's code and keeps working.

All sizes, words and colours are `sidebar.ron`; the picture of an item is data (below). The layout is
`sidebar_layout::layout`, a pure function of the window, the scale and the look
(`the_column_lies_on_the_right_edge_above_the_minimap`, `the_parts_stand_top_to_bottom_money_power_tabs_items`,
`the_column_is_laid_out_in_physical_pixels_at_any_scale`, `a_tiny_window_gets_no_item_rows_and_does_not_panic`).

## The edge band (progress item 22, decided)

The window edge scrolls the camera in a band of `edge_margin` physical pixels (`input.ron`, 4), and the cursor shows
the scroll arrow there (`cursor.md`, row 1: the band wins over everything). Two decisions:

* **The sidebar stays out of the band.** Its margin to the right and top edge is `margin_px` (6 logical pixels) and
  `layout` never makes it less than the band, even at a UI scale where 6 logical pixels are fewer than 4 physical ones
  (`the_sidebar_never_covers_the_edge_scroll_band`: scales 0.5 to 3; `the_edge_band_and_the_sidebar_do_not_fight`: a
  press in the last four pixel columns is not the UI's, one on the last column of the sidebar is). So a cursor is on the
  sidebar or in the band, never both: over the sidebar the camera does not scroll, in the band it does. The band width
  is read from `input.ron` when the HUD starts (a change of `edge_margin` while the game runs is not picked up).
* **A right-click on a unit inside the band still gives no order.** This is kept on purpose and documented, not
  changed: the scroll arrow is on the cursor there, and a right-click does exactly what the cursor shows (`cursor.md`,
  one source of truth). Letting the band yield to a unit under the cursor would change `cursor_context` for every
  screen edge and break that rule. It is a band of four pixels, and the sidebar no longer sits in it.

## Which items a tab shows

An item is a role with a price (`cost`) that the local faction may have (`factions`), on the tab of its category
(structure, infantry, vehicle, vessel; an object has no tab). The list is made by `Catalog::build` from the unit
definitions, once, and filtered each frame by what stands (`items_for`):

* a **structure** shows when every structure of its `building.requires` stands;
* a **unit** shows when a factory of its function stands (infantry: infantry factory, and so on, `produced_by`) and
  every structure of its `requires` stands.

*Stands* is the simulation's word: finished and with hit points. A structure still going up counts for nothing
(`a_structure_that_is_still_going_up_does_not_count_as_standing`, `a_unit_tab_needs_a_standing_factory_of_its_function`),
and only the local player's structures count (`only_the_local_players_structures_count`).

**What is not available is hidden, not greyed.** The tech tree (`buildings.md`) is the way the player finds what
exists: a tab that lists only what can be built now shows the tree opening as structures stand. A tab with nothing says
what it misses ("Needs a barracks") or, when the factory stands and the faction simply has no such unit (Meridian has no
vessel), "Nothing to build" (`an_empty_tab_names_the_factory_it_misses_or_says_there_is_nothing`). Credits do **not**
lock an item: a structure or unit the player cannot pay yet can still be chosen or queued, and waits for the money as
production and construction already do (the price is shown in the warning colour,
`a_price_the_player_cannot_pay_is_marked_but_the_item_stays`).

**The order is fixed and comes from data**: the `order` of the `build_picture` of the record, ascending within a tab
(`the_order_follows_the_order_of_the_pictures_not_the_kind_numbers`, which swaps two roles by editing the data and
needs no code change). Roles without a picture follow after those that have one, by kind.

## What an item says

Everything is read from the snapshot of the simulation; nothing is counted in the presentation.

* The **picture** (a coloured tile with the short name), the **price** in credits (bottom left).
* **In progress:** a bar along the bottom edge of the tile, as long as the progress in thousandths
  (`the_progress_bar_is_as_long_as_the_progress_and_turns_to_the_waiting_colour`), and the percentage on the right
  of the price line.
* **The count** in the top right corner: of a structure, how many of its kind are going up; of a unit, how many of its
  kind are in the queue, the one being built included (`a_queued_unit_shows_its_own_progress_and_count`,
  `a_structure_going_up_shows_its_own_progress_and_count`: two different kinds with different progress, so a mix-up
  shows).
* **Waiting:** the bar turns to the waiting colour and the right of the price line says why, in the words of
  `production_view` (WP6.5): "no credits" (`WaitingForMoney`, or a structure under construction that is stalled),
  "exit blocked" (`WaitingForExit`), "stopped" (the factory's speed is 0, the hook of WP6.4), "needs <structure>"
  (`MissingPrerequisite`). It is shown on the item being built, the front of the queue, and not on the ones behind it
  (`a_unit_that_waits_for_credits_says_so_on_the_item_at_the_front_only`,
  `a_finished_unit_with_no_free_exit_says_the_exit_is_blocked`,
  `a_structure_that_waits_for_credits_says_so`).
* **The chosen structure** (the build mode is on for it) has a ring round its tile
  (`the_chosen_structure_gets_a_ring_and_the_others_do_not`).

The values that are the data's own are checked against the simulation's tables, not against the same data read twice:
`every_entry_has_the_price_and_time_the_simulation_will_charge` compares each entry with
`ProductionMenu::price_of` (the unit table) and the building table of the world.

## Clicks

A press acts when the button goes down. Only the left and the right button do anything.

| On | Left | Right |
|---|---|---|
| a tab | shows the tab (the scroll goes back to the top) | nothing |
| a **structure** item | starts the build mode (WP6.2) for that structure: the ghost follows the cursor and the left click on the map sends `BuildStructure`, exactly as after the key B | puts the chosen structure away (the build mode ends) |
| a **unit** item | `QueueUnit { factory, kind }` | `CancelUnit { factory, index }` for the **last** item of that kind in the queue; nothing when none is queued |
| anything else on the column | nothing | nothing |

(`a_click_on_a_structure_chooses_it_and_a_right_click_puts_it_away`,
`a_click_on_a_unit_queues_it_and_a_right_click_cancels_the_last_one_of_its_kind`, which runs each command in a world
and looks at the queue; `the_sidebar_chooses_a_structure_by_kind_and_the_click_places_that_kind` for the build mode.)
The commands are the ones WP6.5 already has; the simulation checks each again at its tick and records a refusal. The
free harvester of a refinery is in the queue but cannot be taken out, so a right-click skips it
(`the_free_harvester_of_a_refinery_cannot_be_cancelled_from_the_sidebar`).

**The primary factory.** A unit goes to the **selected** own factory of its function (the lowest slot if several are
selected), else to the standing factory of that function with the **lowest slot**. A selected factory of another
function does not matter: each tab has its own (`the_primary_factory_is_the_selected_one_else_the_lowest_slot`). The
progress and the count of an item are those of its primary factory's queue.

**Clicks never reach the map.** The whole column is one blocking panel, so a press anywhere on it is the UI's: no
selection box starts, no order is given behind it, and the build mode's click does not place anything with a stale
cell (`a_press_on_a_structure_chooses_it_and_the_ui_keeps_the_click`,
`a_press_on_the_column_between_the_parts_is_consumed_and_does_nothing`,
`a_right_press_on_a_structure_puts_the_chosen_one_away_and_orders_nothing`: `press_consumed`, `pointer_over_ui`
and `button_captured` are what `handle_selection`, `handle_orders` and `handle_build` already ask). The same press
outside the column is not consumed. The keys stay as they were; U, I, O and G of WP6.5 remain as debug keys.

## The picture: `build_picture` in `unit_defs.ron`

```ron
build_picture: (color: (230, 190, 60), label: "PWR", order: 10),
```

A field of its own, **separate from `icon`** (the name of the strategic map icon since WP3.6). Until there is art the
picture is a coloured tile with a short name drawn from this data. Checked when the definitions load:

* only a role with a `cost` may have one (only those are built);
* `label` is 1 to 6 printable ASCII characters without a space at either end;
* `order` is 0 to 999 and unique among the roles of one category (the same number in two tabs is fine: each tab has
  its own list).

The field is **optional**, which keeps the frozen copies in `assets/scenarios/*_defs/` loading unchanged (none was
touched). A role without one gets the default picture of its tab (`sidebar.ron`, `default_colors`), the first
`default_label_chars` letters of its id in capitals, and comes after the roles that have a picture
(`a_role_without_a_picture_gets_the_default_of_its_tab_and_comes_last`). Every shipped role with a price has a picture
(`every_shipped_role_with_a_price_has_a_picture_of_its_own`). Tests of the checks: `crates/sf_data/tests/build_picture.rs`.

## Performance

The sidebar costs the simulation nothing: it reads the snapshot. On the main thread each frame it does one pass over
the entities of the snapshot (`Situation::read`), builds the items of one tab and a few hundred quads on the UI.
`crates/sf_game/tests/sidebar_cost.rs` bounds the quads (`the_number_of_quads_is_bounded_and_does_not_grow_with_the_world`:
the same quad count with 0 and with 5,000 extra entities) and has an `--ignored` timing; the numbers are in
`docs/progress.md`.

## Not done, on purpose

Power (WP6.4: the hook is there, nothing fills it); the repair and sell buttons (WP6.6); menus and settings
(WP11.5); click sounds (audio is another work package); an icon picture instead of the coloured tile (art, WP11.6);
a tooltip with the full name and the build time (the tile shows the short name and the price); hotkeys for the tabs;
a rally-point button; showing the other factories' queues (an item shows its primary factory's queue only); a
second queue for structures (a structure is placed at once and goes up where it stands, WP6.2/WP6.3); a faction
other than the local seat's (the catalog is built for the faction of `LOCAL_PLAYER`).
