# WP6.6 screenshots (repair and sell)

Written by `crates/sf_game/tests/repair_screenshot.rs` (`SF_SAVE_SHOTS=$PWD/docs/screenshots/wp6.6 xvfb-run -a cargo test -p
sf_game --test repair_screenshot`, an absolute path), real renderer on a software adapter (llvmpipe, under Xvfb), 1280 x 720, the
game's own world (`sf_game::playfield`: units, buildings, money, production, power, repair, combat) on the playfield map with the
sidebar, the cursor, the health bar of the selected structure, the repair sign and the sale bar made by the functions the game
calls. The number after `t` in a name is the number of ticks the world has run. Everything is found in the simulation: the
structures are boxes because the harness builds the roster without glTF models (as the WP6.4 shots do). The cursor is drawn at the
screen position of the structure; in the game it is the mouse.

* `repair_mode_cursor_t00394_hp285.png`: **a hurt plant and the Repair mode.** Four tanks shot the power plant down to 285 of 600
  hit points and were removed. The Repair button is lit (the other button is not), the cursor over the plant is the spanner, and the
  health bar of the selected plant is short. Credits 1000, power 50 used of 47 made (a plant at 285 of 600 makes 47).
* `hurt_t00394_hp285.png`: the same moment with the mode off, for the comparison.
* `repairing_t00435_hp405.png`: **under repair.** 40 ticks after the tick of the click (the command ran in tick 394, 3 hit points a tick from
  the next one): 405 hit points, 120 gained. The repair sign (a plus on a badge) floats on the plant, the health bar is longer, the credits are
  985: 405 / 8 - 285 / 8 = 15 credits, by hand (a whole repair of a 300-credit plant costs 250 thousandths, so a hit point costs 1/8
  credit).
* `repaired_t00500_hp600.png`: **done.** 600 of 600 hit points at tick 500 (the last tick gained 3), no sign, the whole cost paid
  (75 - 35 = 40 credits) and no more, and the plant makes its 100 power again from the next count.
* `sell_mode_cursor_t00001.png`: **the Sell mode.** The Sell button is lit and the cursor over the barracks is the coin.
* `selling_t00042_40_percent.png`, `selling_t00092_90_percent.png`: **a sale in progress.** The sale bar under the barracks
  (100 ticks in all) is two fifths and then nine tenths full; the pixel count of the fill grows by the ratio 2.25 (checked in the
  test). The sidebar has lost the items the barracks unlocked and the power bar says 30 used (the barracks no longer uses 20).
* `sold_t00103.png`: **gone.** The sale started in tick 1 and the barracks was removed in tick 101, the refund of 200 credits (400 * 500 / 1000 at full hit points)
  is in the store, no bar.

Not shown: a repair that waits for money (its sign turns to the warning colours; the colours are in `repair_look.ron`, the state is
tested in `sf_sim` and in the headless scenario `repair_sell`), and a run of the real game (the sandbox has no scripted attackers to
hurt a structure; the Repair mode on a structure hurt by a player's own game needs an enemy to reach it).
