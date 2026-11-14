# WP9.2a-2: the gather cell off the ore (screenshots)

`m1_ai`, the AI on the north-east site against an idle human on the south-west one, the game seat's picture with every view
drawn and the camera on the middle of the AI's army (`crates/sf_game/tests/ai_screenshot.rs`, `an_ai_attack_wave_in_the_games_picture`;
`SF_SAVE_SHOTS=<abs path> xvfb-run -a cargo test -p sf_game --test ai_screenshot an_ai_attack`, debug build, Xvfb with the software adapter).
The file names carry the tick.

| File | What it shows |
|---|---|
| `before_m1_ai_t09600_attack_wave_all_views.png` | **Tick 9,600, rules off** (`gather_refinery_gap` 0, `gather_everyone` false; the profile as it was at 2cacd4b). The gathered army (tanks and riflemen) stands **in the middle of the AI's ore field** (the orange ground), among the harvesters and in front of the refinery (top): the old rule picked a gather cell on the ore (in `m1_duel` on the same site it was the ore cell (66, 26)). |
| `after_m1_ai_t09600_attack_wave_all_views.png` | **Tick 9,600, shipped rules** (gap 2, every unit gathered). The army (a tank and riflemen at the edge of the field, the rest further off the frame) stands on open ground **beside** the ore field (in `m1_duel` on the same site the cell is (68, 28)), at least two cells from the refinery; the one vehicle on the ore is a harvester. |
| `after_m1_ai_t10600_attack_wave_all_views.png` | Tick 10,600: the strike (launched at 10,037) on its way to the staging cell. |
| `after_m1_ai_t12400_attack_wave_all_views.png` | Tick 12,400: the wave at the human's construction centre, 273 ticks before it falls (12,673). The test asserts that at least 4 of the AI's units are within 12 cells of it (6 measured) and none at 9,600. |

Not shown: the tournament numbers (tables in `docs/rules/ai.md`, "The start-site advantage"), which are about many games, not one picture.
