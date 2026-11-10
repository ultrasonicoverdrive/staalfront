# WP7.7 screenshots (the loss rule by construction centre)

Written by `crates/sf_game/tests/match_screenshot.rs` (`SF_SAVE_SHOTS=<absolute path to this directory> xvfb-run cargo test -p
sf_game --test match_screenshot`), real renderer on a software adapter (lavapipe, under Xvfb), the pinned scenario `match`
(gate on, the opponent has its construction centre). The tick is in the name; red is the human, blue the opponent.

* `match_t00002_start_enemy_base.png`: the opponent's base at the start. The big blue block at the bottom is its new
  **construction centre** (4 by 4 cells at (72, 14)); the tower, the riflemen, the plant and the ore field are where they were.
* `match_t00002_start_whole_map.png`, `match_t00002_start_base.png`, `match_t02300_*`, `match_t03940_fight_at_the_enemy_base.png`:
  as in WP7.6 (the whole map, the human's base, the army at the rally point, the fight at the opponent's tower).
* `match_t06239_victory_banner.png`, `match_t06239_defeat_banner_for_the_opponent.png`: the step after tick 6238, the tick the
  opponent's construction centre fell. The banner says "Victory" and "decided after 5:11 (tick 6238)": its text is unchanged, only
  the moment moved. The test asserts that no banner exists before the result is decided.
