# The sidebar

Work package WP11.1 (PLAN.md phase 11), built before M1 by the orchestrator's decision of 01-11-2026: the
debug keys of WP6.5 (U, I, O, G) are not a way to play. Presentation only (WP6.4 later filled the power bar and added the low-power sign, below): no simulation code changed, no new
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
   **The strip "build paused: no credits" of the old readout is gone from the column** (decided in WP11.1, fix
   round 1): the column has no room for a second line, and the same news is shown twice already: the credits figure
   turns the warning colour while any build of the player waits for credits (`SidebarModel::waiting_for_credits`, also
   when the tab of the waiting item is closed), and the waiting item itself says "no credits" with the bar in the
   waiting colour. A structure going up on the map keeps its pause strokes (`economy.md`, "Paused bar").
2. **The power bar.** `sidebar::power_reading` returns the local player's `PowerReading { made, used }` from the snapshot
   (WP6.4: the totals the simulation counted, `power.md`; `the_power_bar_reads_what_the_simulation_counted`), and `None`
   in a world without power rules, where the bar draws a neutral frame saying "Power: no power rules". The bar fills with
   what is used against the larger of made and used, in the low colour when more is used than made (`power_fill`, tested;
   the drawing of both states is `the_credits_the_power_bar_and_the_tabs_are_drawn`; shots in `docs/screenshots/wp6.4/`).
3. **Four tabs**: Structures, Infantry, Vehicles, Vessels, two by two. The tab shown is lit; a tab with nothing to
   build has dimmed text but can still be opened.
4. **A grid of items**, two per row, as many rows as fit. The wheel over the column scrolls it a row at a time
   (the wheel there is not a camera zoom).
5. **The minimap** stays where it was, bottom right, 200 px wide, under the column (`bottom_reserve_px` is the room
   it takes). It is not part of the sidebar's code and keeps working.

All sizes, words and colours are `sidebar.ron`, down to the credits figure's size and inset
(`credits_text_pct`, `credits_inset_px`), the strip under a picture (`strip_pct`), the frame round a picture
(`picture_inset_px`), the padding of the count and the strip text (`text_pad_px`) and the ring of the chosen structure
(`chosen_ring_px`), all pinned in `the_shipped_look_is_pinned`; the halves and doubles left in `sidebar_view.rs` are
geometry (centring, both sides of a padding), not looks; the picture of an item is data (below). The layout is
`sidebar_layout::layout`, a pure function of the window, the scale and the look
(`the_column_lies_on_the_right_edge_above_the_minimap`, `the_parts_stand_top_to_bottom_money_power_tabs_items`,
`the_column_is_laid_out_in_physical_pixels_at_any_scale`, `a_tiny_window_gets_no_item_rows_and_does_not_panic`).

## The loop button (WP6.10)

A third button in the row of Repair and Sell, **Loop** (`sidebar.ron`: `words.repeat`; the colours are the tool colours). It acts on the
**primary factory of the open tab** (`Situation::primary`: the selected one, else the lowest slot) and is **lit while that factory repeats**
(`SidebarModel::repeat`, a `RepeatButton { factory, on }`). A left click sends `SetRepeat { factory, on: !on }`
(`RepeatButton::click`); a right click does nothing. On the structures tab, and on a unit tab whose factory does not stand, the button is
dim and silent. It does not depend on the repair rules. A queued item of a repeating factory wears a small **loop sign** at the top left of
its picture (`words.repeating` at `repeating_px`, 8 px; `Item::repeating`; the same colour as the lit button). The label of that item is centred in
the picture **below** the sign (`sidebar_view::label_rect`, `loop_sign_rect`), so the sign never covers a letter; the test
`the_loop_sign_never_covers_the_label_of_its_picture` calls both functions for every label of the shipped catalog at 1280 x 720, 1920 x 1080 and
800 x 600, scale 0.5 to 2, and checks that the boxes do not meet (at scale 1 and above the label also stays inside the picture; at scale 0.5 the picture is too small for
sign and label and only the first claim holds). `picture_rect` and `count_badge_rect` are the functions `draw_tile` and the test share. **Gap, measured**: a count badge of
one digit never reaches the moved label (all labels, scale 1 to 2), but with a count of ten or more the badge reaches into the glyph box of
DECOY, ROCKT and SCOUT (1280 x 720, scale 1; the set is pinned in the test); nothing was widened for it. The screenshot test checks the pixels. Mouse only: no key in this WP (letters such as R and L are free in `input.ron`, but a key needs a decision with the
rest of the key map; the button is one click). The row is three equal buttons now. Tests: `sidebar/tests.rs`
(`the_repeat_button_acts_on_the_primary_factory_...`), `sidebar_view/tests.rs` (press, lit, sign), `sidebar_layout/tests.rs`
(`the_repeat_button_is_the_third_in_the_row_of_buttons`), `tests/sidebar_screenshot.rs` (pixels from a real world;
`docs/screenshots/wp6.10/`: `sidebar_t00009_repeat_off.png`, `sidebar_t00277_repeat_on.png` with four riflemen out and two in the queue).

## The Repair and Sell buttons (WP6.6)

A row of two buttons under the power bar, **Repair** and **Sell** (`sidebar.ron`: `tool_height_px`, `tool_labels` and the colours `tool`,
`tool_hover`, `tool_active`). They are mode buttons: a left click switches the mode on (the button is lit; the other one goes out; a chosen
structure to build is put away), or off when it is the mode that is on; a right click on a button leaves the mode. With a mode on, a left click on
the player's own structure on the map sends `Repair` or `Sell` and never starts a selection (`a_left_click_in_a_repair_or_sell_mode_starts_no_selection`), and a right click or Esc leaves the mode (`the_right_click_that_leaves_a_repair_or_sell_mode_orders_nothing`, `esc_ends_a_repair_or_sell_mode_before_it_opens_the_pause_menu`; `repair_sell.md`, "The picture";
`cursor.md`, "Modes"). Rows: the row sits between the power bar and the tabs, so the item grid is 24 and a gap shorter than in WP11.1 (five rows
still fit in the default window, `the_parts_stand_top_to_bottom_money_power_tabs_items`). A structure being sold does not stand, so the
tabs and items it unlocked disappear from the first tick (`Situation::read`). In a world without repair rules the buttons are drawn dim and do
nothing. Tests: `sidebar_view/tests.rs` (clicks, lit button, dim buttons), `sidebar_layout/tests.rs` (the row and the hit test).

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

## A new scene

The tab and the scroll belong to the HUD, which a Restart keeps. `Runner::replace_scene` puts them back on the default tab
(Structures) at scroll 0 (`Hud::reset_sidebar`), for Restart, Main menu and a new scene alike
(`a_restart_puts_the_sidebar_on_its_default_tab_at_the_top`).

## Which items a tab shows

An item is a role with a price (`cost`) that the local faction may have (`factions`), on the tab of its category
(structure, infantry, vehicle, vessel; an object has no tab). The list is made by `Catalog::build` from the unit
definitions, once, and filtered each frame by what stands (`items_for`):

* a **structure** shows when a construction centre of the player stands (any `Anchor` kind: the simulation places
  nothing outside the build radius of a standing anchor, `buildings.md`) and every structure of its
  `building.requires` stands. Without a standing anchor the structures tab is empty and says "Needs a construction
  centre" (`empty_hints.0`), while the unit tabs of standing factories keep working
  (`without_a_standing_construction_centre_no_structure_is_offered`);
* a **unit** shows when a factory of its function stands (infantry: infantry factory, and so on, `produced_by`) and
  every structure of its `requires` stands.

*Stands* is the simulation's word (`World::is_standing`): finished, and with hit points left or of a kind that
cannot be damaged (0 HP in the unit table). A structure at 0 HP unlocks nothing and is never the primary factory, even
when selected (`a_factory_at_zero_hit_points_unlocks_nothing_and_is_never_the_primary`). A structure still going up counts for nothing
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
  "exit blocked" (`WaitingForExit`), "stopped" (the factory's speed is 0: no power at all and a shortage floor of 0, WP6.4), "needs <structure>"
  (`MissingPrerequisite`). It is shown on the item being built, the front of the queue, and not on the ones behind it
  (`a_unit_that_waits_for_credits_says_so_on_the_item_at_the_front_only`,
  `a_finished_unit_with_no_free_exit_says_the_exit_is_blocked`,
  `a_structure_that_waits_for_credits_says_so`).
* **Low power (WP6.4):** an item that is being built, does not wait and goes on at less than full speed because its
  player is short of power says "low power" in the warning colour in the place of the percentage, and its bar is the amber
  `bar_low` colour (`Item::low_power`, from `Situation::slowed`, which is the snapshot's `power_speed` below 1000): the same
  place and manner as "no credits", with a bar colour of its own because the item is advancing, only slowly
  (`an_item_that_is_being_built_in_a_shortage_says_low_power`, `a_structure_going_up_in_a_shortage_says_low_power_and_one_with_enough_does_not`,
  `an_item_in_a_power_shortage_has_its_own_bar_colour_and_says_so`). A structure item says it too while it goes up.
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

**The tab follows the selection (WP6.9).** When the selection **changes** and holds an own factory that stands, the
sidebar shows the tab of that factory (barracks: Infantry, vehicle factory: Vehicles, shipyard: Vessels) at the top of
its items, as a click on the tab would. With several factories selected it is the tab of the **first one in the stable
selection order** (ascending by id), which is also the primary factory of that tab. Code: `sidebar::TabFollow`
(remembers the selection it last saw and answers `Some(tab)` only when the selection differs from that and has a factory
with a unit tab), called once per frame by `Runner::follow_selection_tab` after the selection and order handlers;
`Hud::show_sidebar_tab` sets it. It acts **only on a change of the selection**, never every frame, so a tab the player
picks by hand stays until the selection changes again; a selection of units only, of a power plant or of nothing leaves
the tab as it is; an inspected enemy factory is not in the selection and does not move it. The selection changing
because one of its members died counts as a change. Tests: `rally/tests.rs`
(`the_tab_follows_a_selection_that_changes_to_a_factory_and_only_then`: 100 further frames with the same selection answer
nothing, a different factory switches, units/plant/empty answer nothing, a mixed selection follows its factory;
`an_enemy_factory_inspected_does_not_move_the_tab`), the wiring by reading `Runner::follow_selection_tab` and in the
screenshot `docs/screenshots/wp6.9/`.

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

Click sounds (audio is another work package) (menus and settings exist since
WP11.5a: `menus.md`; the UI scale of the settings multiplies everything here, the sidebar keeps out of the edge band at every
scale it offers, and while a menu is open the sidebar is built blind: no hover, no click); an icon picture instead of the coloured tile (art, WP11.6);
a tooltip with the full name and the build time (the tile shows the short name and the price); hotkeys for the tabs;
a rally-point button (the rally point is set by right-click since WP6.9, `production.md`); showing the other factories' queues (an item shows its primary factory's queue only); a
second queue for structures (a structure is placed at once and goes up where it stands, WP6.2/WP6.3); a faction
other than the local seat's (the catalog is built for the faction of `LOCAL_PLAYER`).
