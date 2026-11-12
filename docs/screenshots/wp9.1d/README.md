# WP9.1d screenshots (the computer opponent in the game)

The match is `assets/matches/m1_ai.ron` (`sf_game --match m1_ai`): the human (Meridian, south-west) is idle, player 1 (Veyran, north-east) is
`sf_ai` with the profile `normal`, thinking on the game's sim thread. The number after `t` in a name is the tick of the snapshot the picture shows.

| File | What it shows |
|---|---|
| `m1_ai_t00002_ai_base_all_views.png`, `m1_ai_t00002_whole_map_all_views.png` | **The start.** The AI has its construction centre and nothing else (top of the close-up; the whole map shows the two centres far apart). |
| `m1_ai_t01000_ai_base_all_views.png` | **Tick 1000.** 4 structures (counted by the test and printed; their names here are read off the picture and the goal list, not asserted): the construction centre, a power plant, a refinery and the vehicle factory (the first three goals of its list since WP9.1c-3, built one at a time on 4000 credits; the factory is ordered at 817). |
| `m1_ai_t02500_ai_base_all_views.png` | **Tick 2500.** 5 structures (the second power plant joined, ordered at 2,177). |
| `m1_ai_t04000_ai_base_all_views.png`, `m1_ai_t04000_whole_map_all_views.png` | **Tick 4000.** 6 structures round the construction centre (placeholder models, blue is Veyran; the barracks, ordered at 3,017, joined), the ore field to the east; the whole map shows both bases. |
| `game_window_t04062_human_view.png` | **The real game window** at tick 4062 (`xvfb-run target/debug/sf_game --match m1_ai --shots DIR --shots-tick 4000`, 1280 x 720, whole map): what the human sees. Only the ground round its own base is known; the AI's base is behind the fog, as it should be (the AI sees only what its own units and structures see, and the human likewise). |

The `m1_ai_*` pictures draw **every** view, seen or not (as `match_screenshot` does), because they are made to show what the AI did; in the game the human
would not see it (the test asserts that the human sees none of the AI's things at tick 4000). Made by `crates/sf_game/tests/ai_screenshot.rs` with
`SF_SAVE_SHOTS=docs/screenshots/wp9.1d xvfb-run cargo test -p sf_game --test ai_screenshot`.

*Re-taken in WP9.1c-3 (the opening: the vehicle factory second in the goal list): ticks 1000, 2500 and 4000 changed (4, 5 and 6 structures; they were 4, 6 and 6, with the barracks as the third goal), and so did the whole map at 4000; the start pictures and the game window did not (the 1000 file shows the vehicle factory where the barracks was).*

What they do not show: the AI making combat units or attacking (since WP9.1c-1 it buys a scout and, once its goal list is done, units, but in `ai_scout` (4,000 credits) the units come after tick 15,000, and the army does not attack before WP9.1c-2). Made after the merge with the economy manager (WP9.1b-2): it queues harvesters, which cost money, so it stands at 6 structures at tick 4000 where it stood at 8 before.
