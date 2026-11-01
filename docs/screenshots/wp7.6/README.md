# WP7.6 screenshots (the first match)

Written by `crates/sf_game/tests/match_screenshot.rs` (`SF_SAVE_SHOTS=$PWD/docs/screenshots/wp7.6 cargo test -p
sf_game --test match_screenshot`, an absolute path, since the test runs in the crate directory), real renderer on a
software adapter (lavapipe, under Xvfb). The tick is in the name. The world is the pinned headless scenario `match`
(`assets/scenarios/match.ron`: the human builds a power plant, a barracks and a refinery, harvests, makes four riflemen
and five rocket troopers and attacks), drawn with the models of the roster, the credits readout (`money_view::draw_panel`)
and, once the match is decided, the banner (`match_view::banner_of` and `draw_banner`: the functions the HUD calls).
Red is the human (Meridian), blue the opponent (Veyran). The `sf_game_*.png` files, if present, come from the real window
(`sf_game --match --shots`).

* `match_t00002_start_whole_map.png`: the whole map 2 ticks in. South-west the human's construction centre (red, 4000
  credits in the readout); north-east the opponent's base (blue: power plant, refinery, barracks, a defence tower, four
  riflemen and a light tank); the lake in the middle with a rock ridge on each side of it, a few rock blocks and the six
  ore fields (brown with yellow flecks).
* `match_t00002_start_base.png`, `match_t00002_start_enemy_base.png`: the two bases close up at the start.
* `match_t02300_base_and_harvester.png`: tick 2300: the credits readout says 413 (the base, the refinery and the units
  cost 3587 so far); the human's harvester works the ore field east of the refinery.
* `match_t02300_army_at_the_rally_point.png`: the human's riflemen and rocket troopers stand at the rally point (34, 60).
* `match_t03940_fight_at_the_enemy_base.png`: tick 3940, 250 ticks after the first death: the human's riflemen (red) stand
  on the opponent's ore field in front of its light tank and defence tower (blue), which fire back.
* `match_t05688_victory_banner.png`: the step after tick 5687, the tick the opponent's last structure fell: the banner
  says "Victory" and "decided after 4:44 (tick 5687)"; the army stands in the ruins of the opponent's base.
* `match_t05688_defeat_banner_for_the_opponent.png`: the same tick for the other player: "Defeat" in the colours of a
  defeat.

Not shown: a person playing the match in the real window (a click cannot be made from outside the window).

* `sf_game_match_t00137_close_up.png`, `sf_game_match_whole_map.png`: the **real game window** (`xvfb-run sf_game --match --shots <dir>`,
  debug build, lavapipe) 137 ticks in: the match map with `Credits 4000`, the minimap, the opponent's ten objects as strategic
  icons (blue) and the human's construction centre (red) with the camera opened on it. A few small marks (on the lake, and a magenta square near the ridge) are not explained: I did not identify them; they are
  not among the 11 units the top bar counts (an open question for the reviewer).
