# WP6.4 screenshots (power)

Written by `crates/sf_game/tests/power_screenshot.rs` (`SF_SAVE_SHOTS=$PWD/docs/screenshots/wp6.4 xvfb-run -a cargo test -p
sf_game --test power_screenshot`, an absolute path, since the test runs in the crate directory), real renderer on a software
adapter (lavapipe, under Xvfb), 1280 x 720. The tick is in the name. The harness draws with the functions the HUD calls
(`sidebar::power_reading`, `sidebar::Situation::read`, `sidebar::model`, `sidebar_view::frame`) over the playfield: a
construction centre (84, 74), a power plant (78, 74), a barracks (84, 80) and a refinery (96, 86) of player 0 with the shipped
1000 credits, drawn with the box placeholders of the roster. The sidebar is the column on the right (the harness draws neither
the top bar nor the minimap). The power bar reads "used/made". The harness does not draw the damage stages of the models
(WP6.8 does, in `docs/screenshots/wp6.8`): the damaged plant shows in the numbers.

* `power_t00032_fine_infantry.png`: **power fine.** A plant (+100) against a refinery (-30) and a barracks (-20): the bar
  reads "Power: 50/100", half full, in the normal green. The rifleman at the front of the barracks queue (count 2) is 30 percent
  along, with the normal blue bar and its percentage.
* `power_t00053_shortage_infantry.png`: **shortage.** A vehicle factory (-40) and a radar (-50) have been put up: 140 used of
  100 made, speed 714. The bar is full and in the low colour ("Power: 140/100"). The rifleman is half done, its bar is the amber
  low-power colour and the right of the price line says "low power" where the percentage was.
* `power_t00097_shortage_structures.png`: the structures tab while a second plant (ordered in the shortage) goes up: its bar
  is amber too, and the bar of the power is still red; the plant goes up at the same slow pace.
* `power_t00373_damaged_plant.png`: **a damaged plant.** Four Veyran light tanks (the blue cones, top left) have shot the plant
  down to 200 of 600 hit points: it makes 100 * 200 / 600 = 33, the bar reads "Power: 50/33" and is in the low colour, with
  nothing but the plant's damage to blame (the barracks and the refinery use the same 50 as before).
* `game_t00065_sandbox.png`: **the real game** (`sf_game --sandbox --harvest --shots-tick 47 --shots <dir>` under Xvfb on
  lavapipe, release build, 1280 x 720, close-up shot at tick 65): the HUD's own sidebar with the power bar filled from the
  simulation, "Power: 270/100", in the low colour. The sandbox places one of every structure for each side, so its base is in a
  permanent shortage (270 used of 100 made): everything it builds runs at 370 thousandths. No queue is running at this tick, so
  no item carries the low-power sign in this shot.

Not shown: the low-power sign in the real game (the sandbox has no queue running at a shot tick; the harness shots above show it
through the same functions the HUD calls); a click in the real window (there is no `xdotool` here).
