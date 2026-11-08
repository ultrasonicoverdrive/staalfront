# Menus and settings

Work package **WP11.5a** (PLAN.md phase 11): the start menu, the pause menu with restart, and a settings screen for keys,
graphics and accessibility. The sound settings are **WP11.5b** (decision of the orchestrator, 03-11-2026: the sound is
built later, in agreement with the audio work; the settings screen has a marked empty place for it). Presentation only:
**no `sf_sim` code changed**, no pin moved, nothing in the menus is a command (pausing is not one, see below).
Code: `crates/sf_game/src/{menu.rs, menu_view.rs, menu_look.rs, settings.rs, palette.rs}` (new) with small hooks in
`runner.rs`, `hud.rs`, `args.rs`, `lib.rs`, `sim_thread.rs`; `sf_engine` (bindings and frame cap can be set while the
game runs), `sf_ui` (`Ui::set_input`, `Ui::block_rect`) and `sf_render` (`Style::with_palette`, `Instancer::set_palette`,
`Renderer::set_present`). Data: `assets/config/menu_look.ron`, `assets/config/palette_colour_blind.ron`. Pictures:
`docs/screenshots/wp11.5a/`. Rules in our own words (clean-room, B5); no Red Alert material or look (B11).

## What the player gets

* **The start menu** when `sf_game` starts with no flag that names what to run: **Match** (the M1 match,
  `assets/matches/m1.ron`), **Sandbox**, **Settings**, **Quit**. Behind it stands the sandbox, held at its first tick.
* **The pause menu** on **Esc** in the sandbox or the match: **Resume**, **Restart**, **Settings**, **Main menu**, **Quit**.
  The simulation takes no tick while it is open.
* **After the match is decided** (the Victory, Defeat or Draw banner of `match.md`) two buttons stand under the banner,
  **Restart** and **Main menu**; Esc still opens the pause menu as well. The game keeps running behind the banner.
* **The settings screen** with four tabs: **Keys**, **Graphics**, **Accessibility** and **Sound (WP11.5b)** (marked, with a
  heading, a line of text and no control). It opens from either menu and **Back** (or Esc) returns to the one it came from.
  Every change is applied at once and written to the user's settings file.

All words, sizes and colours are `menu_look.ron` (`sf_game::menu_look`, checked when it parses and pinned by
`the_shipped_look_is_pinned`; `a_look_that_cannot_work_is_refused` tries fifteen broken variants).

## When the start menu shows

Only for the plain `sf_game`. Every existing flag keeps its old behaviour and **skips the menu**, so scripts, the CI smoke test
(`sf_game --shots dir`) and the benchmarks do not change: `--sandbox`, `--match [FILE]`, `--crowd`, `--fight`, `--harvest`,
`--build`, `--build-at`, `--units`, `--bench`, `--shots`, `--exit-after`, and the engine demo `sf_demo` never has one.
(`the_start_menu_is_for_the_plain_game_and_every_flag_that_names_a_run_skips_it`.) Window flags (`--windowed`, `--present`,
`--ra-audio`, `--debug-ui`, ...) are not about what to run and leave the menu in place.

For screenshots and checks: `--menu start|pause|keys|graphics|accessibility|sound` opens that menu. `start` opens at the start
(also with `--exit-after`); the others open over the scene **once the sim has reached `--shots-tick`** (so the picture shows
the game at a tick) and hold it there. In a scene without menus (`--crowd`, a benchmark, the demo) the flag does nothing and
**Esc still ends the program**, as it always did.

## Fullscreen key

F11 (`ToggleFullscreen`) switches the window **only with no menu open** (`fullscreen_key_works(menu_open)`, which the runner calls).
With a menu open, the wait for a key to rebind included (a menu is open then), F11 does nothing: while a key is awaited the press
belongs to the menu, which can take F11 as the new key. The window mode is changed in the settings with the mouse then.
Test: `f11_does_not_switch_the_window_while_a_menu_is_open` (the pure function). Checked by reading only, not by a test: the call
of `fullscreen_key_works` at the F11 line in `runner.rs`, and the load of the shipped `config/input.ron` in `lib.rs::run` (both need
a live window/event loop to reach).

## Esc

Esc is the action `Quit` of `input.ron`, which is also **the menu key and cannot be rebound** (the key list shows it as a
fixed row "Menu / back: Esc", and Esc can never be given to another action). With menus (sandbox and match):

| State | Esc does |
|---|---|
| no menu open | opens the pause menu (and holds the sim) |
| a key is awaited in the key list | ends the wait, nothing else |
| a settings page | goes back to the menu it was opened from |
| the pause menu | closes it and lets the game go on |
| the start menu | nothing (there is nothing under it) |
| no menu open, but a Repair or Sell mode of the sidebar is on (WP6.6) | ends the mode; the next Esc opens the pause menu (`esc_ends_a_repair_or_sell_mode_before_it_opens_the_pause_menu`) |

(`esc_closes_the_pause_menu_and_lets_the_game_go_on`, `esc_goes_back_one_screen_at_a_time_and_the_start_menu_stays`,
`esc_while_a_key_is_awaited_ends_the_wait_and_nothing_else`.) The menus pile up as a stack (`Menu::stack`): the settings lie on
the start or the pause menu. **Back** always leaves the page, also while a key is awaited
(`the_back_button_leaves_the_settings_even_while_a_key_is_awaited`). Esc does not cancel a structure that is chosen to build
(it opens the menu): not done, see the end.

## Pause: no tick, no command, same game

Pausing is **not** a sim command and the world never hears of it. `SimHandle::set_paused(bool)` sends a message to the sim
thread, in order with the commands. While it is held the thread steps **no tick**, neither on its own clock nor for
`advance` (`a_held_sim_steps_nothing_not_even_by_hand_and_goes_on_where_it_was`); commands that are sent meanwhile still reach
the world's queue for their tick. **On release the clock starts again from that moment**, so the wall time spent in the menu is not
owed as ticks (no burst of catching up, no clock reset, no skipped or repeated tick).

`a_paused_run_has_the_checksums_of_an_uninterrupted_one` runs the scenario `orders` in real time (commands in before the
clock starts, so when a command arrives cannot differ between two runs), once straight to tick 150 and once with a hold of
0.9 s after tick 30 (on a 20 ms clock, 45 periods, more than the 20 the sim would forgive as lag): no tick runs while held, no snapshot is published, the clock is
not reset (`clock_resets == 0`), no pass is longer than `1 + MAX_CATCH_UP_TICKS`, and **the checksum after every one of the 150
ticks equals the uninterrupted run's**. The runner's own wiring (`Outcome::Paused` holds the sim, `Resumed` lets it go) is
`pausing_holds_the_sim_and_resuming_goes_on_where_it_was`, which compares the checksum at the same tick of a straight run and
a paused one on a real `Runner` (sandbox).

## Restart and the choice of a scene

**Restart** builds the world again from the same setup (the same match file or the same sandbox with the same flags, so the
same seed and the same starting objects) and plays it from its start. **Match** and **Sandbox** on the start menu build the
chosen scene. The mechanism (`Runner::begin`, `replace_scene`): a fresh runner is built by the same `Runner::with_sim` the
program starts with, and everything that belongs to a scene (the map and ground, the sim thread and its snapshot, the instancer,
the looks, the build mode, the catalog, the selection, the camera, the effects) is taken from it; the window, the renderer, the
audio, the HUD, the settings and the menu stay. **Every field of the runner is named in `replace_scene`**, so a field a later
WP adds has to be put on one side or the other before the code compiles. The old sim thread is stopped and joined as its handle
is dropped; the renderer gets the new terrain, the units' buffers and the minimap again (`attach_scene`).

* A scene nobody has played (the one under the start menu, whose clock never started) is not built again when it is the one
  chosen: the sandbox is simply started.
* A scene that cannot be built (a match file that does not load) is said on stderr and the start menu opens again.
  `--models` packs are handed to the new runner; if the new one fails they are gone (only a scene that cannot be built fails).
* The sounds' "last played" memory is reset for the new game (`FxSound::reset`, which exists for it); nothing of the audio code
  changed, and no menu sound exists.

Tests (`runner.rs`): `a_restart_equals_a_fresh_start` (sandbox and match: a runner plays 25 ticks, in the sandbox with an order
given, then restarts; the new world is at the tick a fresh one starts at, and **its checksum at tick 41 equals a fresh run's**),
`a_restart_keeps_the_scene_and_the_match_file`, `the_start_menu_chooses_the_scene`,
`the_outcomes_for_the_window_are_handed_back_not_swallowed`.

## Input is blocked under a menu

While a menu is open **no click, key or wheel reaches the map, the sidebar, the minimap, the camera or the orders**:

* The HUD builds the game's panels with an input that sees nothing (`menu_view::begin_frame`: no cursor, no button), then gives
  the UI the real input (`Ui::set_input`) for the menu, which also takes the **whole window** as one blocker
  (`Ui::block_rect`), so a click beside the panel is the UI's (`pointer_over_ui`). Tests: `a_menu_takes_the_whole_window_and_the_
  game_under_it_sees_nothing`, `a_press_and_release_on_a_button_gives_its_item` (the game has a button on the same spot),
  `ui::tests::a_blind_input_then_the_real_one_lets_only_the_later_widgets_react`.
* The runner skips `handle_sidebar`, `handle_build`, `handle_production`, `handle_selection` and `handle_orders` while a menu
  is open (and the frame in which it opened or closed), drops a drag that had begun, gives the camera no input (so no keys, no
  wheel and no screen-edge scroll), hides the game cursor (the system cursor is back, `cursor.md`), shows no hover, and does
  not play the click sound for a click on the map. The one thing left alone is the **T** test-sound key and the F1 panels
  (they are debug tools).
* The sim is held while a menu is open, so nothing moves behind it either.

Checked on a real window (`scripts/menus_e2e.py`, Xvfb and lavapipe, real X events): with no menu open **B** gives the build line
and a click on the first tile of the sidebar gives the sidebar line (the controls); with the pause menu open the same keys,
**Ctrl+1**, clicks on the map and clicks on the sidebar produce **no** such line and no tick. With the gating taken out of
`render` the check fails ("13 of 14 checks passed": the build and group lines appear).

## Settings file

`sf_game::settings`: RON, read once at the start, written whenever a setting changes, and replaced by the defaults when it
is missing or cannot be used. **Where:** Linux `$XDG_CONFIG_HOME/staalfront/settings.ron`, else
`$HOME/.config/staalfront/settings.ron`; Windows `%APPDATA%\Staalfront\settings.ron`; macOS
`$HOME/Library/Application Support/Staalfront/settings.ron`; the environment variable **`STAALFRONT_SETTINGS`** names another
file on every system (the tests and the screenshots use it) (`the_settings_file_is_where_each_system_keeps_its_configuration`,
`the_environment_variable_names_the_file_on_every_system`).

* **Missing file** = the first run: the defaults, no warning. **A file that cannot be read or parsed**
  (`this is not ron`, a wrong value, a directory) = the defaults and **one** warning on stderr that says the file is replaced on
  the next change; it is never a panic (`a_bad_file_gives_the_defaults_and_one_warning_and_never_a_panic`).
* **A partial file** keeps what it says; unknown fields (a later version's sound settings) are ignored
  (`a_partial_file_keeps_what_it_says_and_unknown_fields_are_ignored`). **Values the game cannot use** are changed into ones it
  can and said, one warning each: the UI scale to the nearest choice, a frame cap of 0 to "no cap" and one out of 10 to 1000 into
  it, key overrides for the menu key or with no key dropped (`values_the_game_cannot_use_are_changed_and_said`).
* **Writing** goes to a file beside the target that is renamed over it, so a crash cannot leave half a file; a write that fails
  is said once and never stops the game (`a_write_that_fails_is_said_once_and_does_not_stop_anything`). The round trip is
  `a_saved_file_reads_back_the_same_and_the_directory_is_made`.
* **Defaults** are `menu_look.ron` `defaults` (windowed, no frame cap, vsync on, UI scale 1.0, standard colours, no key changed).
* **Who reads it.** The engine demo and any `--bench` run never read or write it; a `--shots` run only through
  `STAALFRONT_SETTINGS` (so a screenshot does not depend on who took it); every other run uses the user's file
  (`which_runs_read_the_users_settings_file`, `the_demo_and_the_benchmark_ignore_the_settings`).
* **Flag over setting over default**: `--windowed`, `--fullscreen`, `--resolution`, `--present` and `--frame-cap` win over the
  settings for that run, and are not written (`the_settings_start_the_window_and_a_flag_of_the_command_line_wins`).

## Keys

The key list shows **the 30 actions of `input.ron` that a key can be given to** (`menu_look.ron` `key_groups`: Camera, Orders,
Building and production, Selection, Window and tools) plus the fixed menu key; `no_action_with_a_key_is_missing_from_the_list`
fails if `input.ron` gains a key action that is not listed (and if the list names one that has no key). Actions that are only on
the mouse, the wheel or the screen edge (zoom, drag, the select and order buttons) are not in the list and cannot be changed
here. The list is cut into pages of two columns that fit the window (`menu_view::fit`; a heading never stands alone at the
foot of a column, `a_heading_never_ends_a_column_and_the_pages_keep_every_row_in_order`).

**Rebinding.** Click the key of a row, press the new one. Decisions, all tested in `settings::tests` and `menu::tests`:

* **What is replaced:** the **first key binding** of the action (with its modifiers, if it had any) becomes the plain new key;
  the action's other keys, mouse, wheel and edge bindings stay (pan left keeps its arrow key and the screen edge when A is
  changed; fullscreen keeps Alt+Enter when F11 is changed) (`rebinding_replaces_the_first_key_and_keeps_the_rest_of_the_action`).
* **A key that another action already has is refused, not swapped.** The row stays waiting and says "W is already used for Pan
  up." (`a_key_that_is_taken_is_refused_with_the_name_of_its_action_and_the_wait_goes_on`). The test is on the trigger whatever its
  modifiers (Enter conflicts with Alt+Enter). There is **no exception**, not even for the pair that shares Shift in `input.ron`
  (queue orders and add to selection): rebinding either of them to Shift is refused, and **Reset** puts the shipped pair back.
  Why refuse and not swap: a swap changes a second action the player did not touch, and a mistaken key press is then not
  visible in the row that was clicked.
* **A hand-edited settings file follows the same rules** (`Settings::sanitize`): an override that gives an action Esc, or a key
  (not one of the action's own shipped keys) that another action has, is dropped as a whole, so the action keeps its shipped
  keys, and the load says so in one warning naming the actions. Overrides are taken in action order; one kept earlier counts as
  the other action's key for a later one (`a_hand_edited_file_cannot_give_esc_or_a_key_another_action_has`).
  The check is against the **`input.ron` the game runs with**, read from disk (`load_from(path, shipped)`; the built-in bindings only
  when the file cannot be read), and again when `input.ron` is hot-reloaded (`Menu::set_shipped` runs `Settings::sanitize_keys`
  against the new file and the runner prints what it dropped). A dropped override is gone from memory; the settings file keeps it
  until the next save (`overrides_are_checked_against_the_input_ron_that_was_loaded`,
  `a_hot_reloaded_input_ron_is_checked_against_the_overrides_again`, `a_reloaded_input_ron_drops_the_overrides_it_now_clashes_with`).
* **Esc** ends the wait and never becomes a binding; the menu key is not in the list.
* **Reset** (per row, enabled only when the row differs from `input.ron`) and **Reset all keys**. Giving an action its
  shipped key back leaves nothing in the file (`giving_an_action_its_shipped_key_back_leaves_nothing_in_the_file`).
* **The shipped `input.ron` is never edited.** The settings file keeps, for each changed action, its whole binding list;
  `Settings::effective_bindings(shipped)` puts those over the shipped ones and the engine runs with the result
  (`Context::set_bindings`; `the_player_s_key_fires_the_action_and_the_old_key_does_not` runs the engine's own `ActionState`
  with the new key and the old one). When `input.ron` is reloaded (debug builds) the player's keys go back over it.
  Checked on the real window: after rebinding "Choose structure" to N, **N gives the build line, B does not, and the same holds
  after the program is started again with the file** (`scripts/menus_e2e.py`).

## Graphics

* **Window**: windowed or borderless fullscreen, applied at once (`Context::set_fullscreen`). The row shows the window as it is
  (F11 changes it without the settings), the choice is what is written and what the next start opens with.
* **Frame cap**: off, 30, 60, 120, 144, applied at once (`Context::set_frame_cap`: the engine loop reads it every frame).
* **Vertical sync**: on or off, applied at once (`Renderer::set_present`: the present mode is chosen from what the surface
  supports, as at the start, and the surface is configured again). On the software adapter of the check, turning it off
  printed `present: Immediate` (`scripts/menus_e2e.py`); the choice of the mode is `Present::choose`, which has its own tests
  in `sf_render`. Nothing here needs "on restart".

## Accessibility

* **UI scale** 75, 100, 125, 150, 200 percent (`ui_scales` in `menu_look.ron`). It multiplies `ui.ron`'s scale (and the window's DPI
  factor) for **everything the UI draws: the sidebar, the top bar, the minimap frame, the strategic icons, the banner and the
  menus**, at once (`Hud::set_user_scale`). **The sidebar stays out of the edge-scroll band at every one of these scales**
  (`the_sidebar_keeps_out_of_the_edge_band_at_every_ui_scale_of_the_settings`, on top of `sidebar.md`'s own test) and every
  menu screen fits the window at every scale in 1280 x 720, 1920 x 1080 and 2560 x 1440 (the one exception: 200 percent in a
  720-pixel-high window is taller than the window; its width still fits)
  (`every_screen_fits_the_window_at_every_scale_the_settings_offer`). The tabs and the title stand still when another page is
  chosen (`the_tabs_and_the_title_stay_in_place_from_page_to_page`). **Found while taking the screenshots:** at 150 percent in a
  1280-pixel window the centred top bar ("Dots: on") lay over the credits of the sidebar; it is now narrowed to the room it has
  (`hud::top_bar_width`, `the_top_bar_does_not_reach_over_the_sidebar`: never over the column at 75 to 150 percent, still 520
  logical pixels at 100 percent). At 200 percent a 1280-pixel window is too narrow for the top bar's least width (300 logical
  pixels) beside the sidebar: use a larger window.
* **Player colours: Standard or Colour-blind safe.** The standard colours stay in `units.ron`; the second set is
  `palette_colour_blind.ron`, the same four players in the same order: vermilion (213, 94, 0), blue (0, 114, 178), yellow
  (240, 228, 66), reddish purple (204, 121, 167). It takes the place of the first four colours of the instancer's style, so it
  colours the models' team parts, the primitives, the strategic icons and the dots of the minimap, live
  (`Instancer::set_palette`: the colour of a unit is read from the style every frame, so nothing is re-bucketed). It does **not**
  touch the sidebar's tile colours, the banner or the credits colour (they are not player colours). The wreck colour
  (index 4) is kept. Pictures: `palette_standard_t00045.png` and `palette_colourblind_t00047.png`.
  **The check** (`crates/sf_game/tests/palette_cvd.rs`): each colour is turned to linear light, run through the
  colour-vision-deficiency matrix of Machado, Oliveira and Fernandes (2009, severity 1.0) for protanopia, deuteranopia and
  tritanopia, and turned to CIELAB (D65); the distance is CIE76 delta E; **the smallest distance between any two of the four
  players, for normal vision and for each deficiency, has to be at least 20**. The four colours are four of the seven of the
  Okabe-Ito palette, chosen by trying all 35 sets of four. Measured (smallest pair, delta E):

  | | normal | protanopia | deuteranopia | tritanopia |
  |---|---|---|---|---|
  | colour-blind safe set | 53.6 | 22.7 | 33.1 | 36.4 |
  | standard set (`units.ron`) | 48.3 | **11.0** (blue and magenta) | 36.1 | 43.4 |

  So the standard set is fine for most vision but its blue and magenta come together under protanopia, and the second set is
  at least 22 apart in all four cases; the test states both (`the_standard_set_is_not_safe_for_protanopia_which_is_why_there_
  is_a_second`), and a test checks that the matrices keep grey grey (a typo in a matrix would make every number meaningless).
  **What this is not:** a screening with the models of full dichromacy (the hardest case of each kind), on flat colours; it is
  not a clinical test and not a check of how the colours read against the terrain or in the dark parts of a model. The
  anomalous (weaker) forms are not simulated. Tell Tijmen if the standard set should change instead.

## Sound (WP11.5b)

A marked place: the tab "Sound (WP11.5b)" with its heading and the line "The sound settings (volume sliders and the original Red
Alert audio switch) come with WP11.5b." and no control (`the_tabs_switch_the_page_and_the_sound_page_is_marked_and_has_no_controls`).
`audio.ron`, `sf_audio` and the sound parts of the runner are untouched by this WP.

## What it costs

The menus cost nothing while closed beyond two calls per frame (`menu_view::begin_frame` and `finish_frame`): **10.4 ns a frame
without them and 14.7 ns with them** (`a_closed_menu_costs_a_frame_nothing_measurable`, `--ignored`, release, 200,000 frames,
best of five alternating runs; the machine had a load average of 2.6 to 3.8 on 4 cores with other agents running). An open menu
draws 89 quads (the start menu) to 1,069 (the key list; most are glyphs), measured by `a_menu_is_drawn_on_the_dimmed_window`. The
sim thread's loop gained one `paused` test and a counter per pass (**not timed on its own**: a tick takes milliseconds).
The scene swap builds a world, which takes what a start takes (about 6 s for the 256-cell sandbox map in a debug build: the map
generator is the cost; the match map is loaded, not generated).

## Verified how

* The unit tests named above; `tests/menu_screenshot.rs` through the real renderer (the dim and the buttons' colours at the
  layout's positions, the banner with its two buttons under it, both palettes' swatches in the choice); the real game on a
  software adapter for every picture in `docs/screenshots/wp11.5a/`; `scripts/menus_e2e.py` (26 checks, real X events).
* **Mutations** (a scratch edit, the test that fails, reverted): the menu closes instead of going back from the settings
  (`esc_goes_back_one_screen_at_a_time...`, `the_tabs_switch_the_page...`); Esc while a key is awaited also leaves the page
  (`esc_while_a_key_is_awaited...`); the conflict check removed (`a_key_another_action_has_is_refused...`,
  `a_key_that_is_taken_is_refused...`, `giving_an_action_its_shipped_key_back...`); a rebind replaces the whole list
  (`rebinding_replaces_the_first_key...`); the sim thread ignores the pause (`a_held_sim_steps_nothing...`,
  `a_paused_run_has_the_checksums...`); the clock not restarted on release (`a_paused_run_has_the_checksums...`, the
  `clock_resets` assertion); Restart does nothing (`a_restart_equals_a_fresh_start`); Paused does not hold the sim
  (`pausing_holds_the_sim...`); the game's panels built with the real input under a menu (`a_menu_takes_the_whole_window...`,
  `a_press_and_release_on_a_button...`, `a_menu_is_drawn_on_the_dimmed_window`); the window blocker removed
  (`a_menu_takes_the_whole_window...` after it was made to look beside the panel: **the first version of that test did not catch
  this mutation**, the game's own button had kept the pointer blocked; it does now); `Ui::set_input` a no-op
  (`ui::tests::a_blind_input...` and two menu_view tests); `--shots` not skipping the menu
  (`the_start_menu_is_for_the_plain_game...`); a flag not winning over the setting
  (`the_settings_start_the_window_and_a_flag...`); the top bar not narrowed (`the_top_bar_does_not_reach_over_the_sidebar`); the
  palette order swapped in the RON (`the_standard_choice_leaves_the_style_alone...`, `the_accessibility_page_sets_the_ui_scale_
  and_the_player_colours`); Match and Sandbox swapped (`every_button_of_the_pause_and_start_menus_does_what_it_says`);
  `Requests::apply` ignoring the bindings (`app::tests::requests_replace_the_bindings_and_the_frame_cap_once`); no spacer before a
  heading (`a_heading_never_ends_a_column...`); the banner's two buttons swapped (`the_banner_buttons_stand_under_the_banner_and_
  answer`); `Instancer::set_palette` discarding its colours (`a_new_palette_colours_the_next_frame...`); the UI scale not
  snapped to a choice (`values_the_game_cannot_use_are_changed_and_said`); and the menu gating taken out of `render`, which no unit
  test can reach (it needs a window): `scripts/menus_e2e.py` fails "keys, clicks and the sidebar under the menu do nothing".

## Not done, on purpose

* **The sound settings** (WP11.5b).
* **Keyboard and gamepad navigation of the menus**: they are mouse-driven (Esc and the key capture are the only keys). No
  tooltips, no confirmation for Quit or Restart (they act at once), no "are you sure".
* **Save and load** of a game (phase 13): the pause menu has no such entry.
* **Esc does not cancel a chosen structure**: it opens the pause menu (the right button still puts the structure away). A Repair or Sell mode of the sidebar (WP6.6) is different: Esc ends it first.
* **No menu sounds**, no animation, no music change in the menus.
* **Resolution choice and exclusive fullscreen**: only windowed and borderless fullscreen (`--resolution` stays a flag). A
  frame cap above 144 or an odd one can be written by hand in the file; the menu offers the five.
* **Window size and position are not remembered.**
* **Editing the settings file while the game runs** is not picked up: it is read once, at the start.
* **Two things the real window check cannot see**: the window mode switch on a system with a window manager (the check has none),
  and the pause menu over a match that has been decided (the banner buttons are tested on pixels and as a state machine, not
  clicked in a finished game, because no scripted way to win a match in the window exists yet).
* A player colour that is also a terrain colour (the second set's yellow on desert) has not been looked at beyond the pictures.
