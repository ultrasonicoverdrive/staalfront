# WP6.9 screenshots (rally by right-click, the tab that follows the selection)

Written by `crates/sf_game/tests/production_screenshot.rs`, test
`the_rally_line_runs_from_the_exit_to_the_flag_while_the_factory_is_selected` (`SF_SAVE_SHOTS=<absolute path to this
directory> cargo test -p sf_game --test production_screenshot`), real renderer on a software adapter (lavapipe, under
Xvfb). The tick is in the name. The harness draws with the functions the game calls (`draw_rally_lines`,
`draw_rally_flags`, `sidebar_view::frame` with the tab that `sidebar::TabFollow` answers for the selection, and the
cursor of `CursorKind::Rally`); the scene is the one of WP6.5 (a barracks at (84, 80), a vehicle factory, a refinery),
with a rally point on (92, 90) for the barracks.

* `rally_t00003_selected_infantry_tab.png`: the barracks is selected. A line runs from its exit cell (the cell east of
  it, the barracks' exit side) to the yellow flag of the selected factory on the rally cell; the sidebar is on the
  **Infantry** tab (Rifle, Rocket, Engineer, Scout) without a click on it; the green flag with its foot on the mouse
  position is the new **Rally** cursor.
* `rally_t00003_not_selected_no_line.png`: the same tick with nothing selected: the flag stays (blue, not selected), the
  line is gone, the sidebar shows its default Structures tab.

The same test run also re-renders the WP6.5 production shots under this directory; those are not kept here (the
originals are in `docs/screenshots/wp6.5/`).

Not shown: the real game window (a selection and a right-click cannot be made from outside the window); the
wiring in `Runner` is checked by reading and by the pure functions it calls (`rally/tests.rs`).
