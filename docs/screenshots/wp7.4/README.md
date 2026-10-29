# WP7.4 screenshots (money)

Written by `crates/sf_game/tests/money_screenshot.rs` (`SF_SAVE_SHOTS=docs/screenshots/wp7.4 cargo test -p
sf_game --test money_screenshot`), real renderer on a software adapter (lavapipe). The tick is in the name.
A power plant (300 credits over 300 ticks) is built by player 0, who starts with 150 credits; the
construction centre stands on the left. The harness draws the roster's models, not the build stages, so the
plant shows as a finished box; its progress is the bar under it.

* `money_t00052_building.png`: 100 credits left, the plant is being paid for; the bar has its normal
  colours, the readout `Credits 100` its normal colour.
* `money_t00153_paused.png`: 0 credits, the plant waits at half progress: the bar is orange with two pause
  strokes at its end, the readout is in the warning colour and the strip says `build paused: no credits`.
* `money_t00946_resumed.png`: the first ore was delivered, the plant goes on, the bar has its normal colours
  again.
* `game_t00199_hud.png`: the real game (`sf_game --sandbox --harvest`, Xvfb + lavapipe, tick 199): the HUD panel in the top right corner says `Credits 1000`, the shipped starting credits. A build could not be started from outside the window, so the paused bar is shown only by the test frames above.
