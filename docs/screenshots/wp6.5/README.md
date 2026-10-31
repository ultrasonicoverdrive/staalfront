# WP6.5 screenshots (production)

Written by `crates/sf_game/tests/production_screenshot.rs` (`SF_SAVE_SHOTS=$PWD/docs/screenshots/wp6.5 cargo test -p
sf_game --test production_screenshot`, an absolute path, since the test runs in the crate directory), real renderer on a
software adapter (lavapipe, under Xvfb). The tick is in the name. The harness draws with the functions the sandbox
HUD calls (`production_view::draw_bars`, `draw_rally_flags`, `panel_lines` + `draw_panel`); a barracks (84, 80, behind the
readout panel on the left), a vehicle factory (90, 74, top) and a refinery (right) of player 0 with the shipped 1000
credits. Barracks: two riflemen queued and a rally point on (92, 90). Vehicle factory: two light tanks queued.

* `production_t00044_building.png`: the barracks is selected: the readout shows `barracks: building`, the front rifleman at
  42 percent with 42 credits paid, the second rifleman waiting, the rally cell and the unit on offer; blue bars under
  both factories; the yellow flag of the selected factory on its rally cell.
* `production_t00462_waiting.png`: the credits are gone (two riflemen 200 + a tank 700 leave 100 for the second tank): the
  vehicle factory is selected and reads `waiting for credits` in the warning colour at 14 percent (99 credits paid), its bar is
  orange, and the barracks' flag is blue (not selected). A rifleman that came out stands by the flag.
* `production_t00462_rally.png`: the same tick with the barracks selected again: the rifleman that left the exit stands on
  the rally flag.

Not shown: the real game window with a factory selected (a selection cannot be made from outside the window); the
real game was only started with `sf_game --sandbox --harvest --exit-after 6` under Xvfb and left cleanly.
