# WP9.1c-2 screenshots (the computer opponent attacks)

The match is `assets/matches/m1_ai.ron` (`sf_game --match m1_ai`): the human (Meridian, south-west) is idle, player 1 (Veyran, north-east) is
`sf_ai` with the profile `normal`, thinking on the game's sim thread. The number after `t` in a name is the tick of the snapshot. Placeholder
models only; blue is Veyran (the AI), red the human.

| File | What it shows |
|---|---|
| `m1_ai_t06000_attack_wave_all_views.png` | **Tick 6000, gathered.** The AI's army (12 riflemen and rocket troopers, counted by the test) round its gather cell between its barracks (top, with the units lined up at its exit) and its refinery on the ore field. The army floor bought them from tick 1,147; the first wave leaves at tick 6,037 (`max_wait`). |
| `m1_ai_t07000_attack_wave_all_views.png`, `m1_ai_t07000_whole_map_all_views.png` | **Tick 7000, on the march.** The wave strung out along the west shore of the lake in the middle of the map, on its `AttackMove` to the human's start site; the whole map shows where that is. |
| `m1_ai_t08000_attack_wave_all_views.png` | **Tick 8000, at the human's base.** The 12 units (at least 4 of them within 12 cells of the human's construction centre: that `>= 4` is what is asserted) beside the human's construction centre (bottom left, red), which falls at tick 8,396: the AI wins (`the_army_timing_in_the_game_seat`). |

Like the WP9.1d pictures these draw **every** view, seen or not, with the camera on the middle of the AI's army (at 8000 half-way to the human's
centre); in the game the human would see the wave only once it is in the human's sight. Made by `crates/sf_game/tests/ai_screenshot.rs`
(`an_ai_attack_wave_in_the_games_picture`) with `SF_SAVE_SHOTS=docs/screenshots/wp9.1c-2 xvfb-run -a cargo test --release -p sf_game --test
ai_screenshot an_ai_attack`. The test asserts that no AI unit is within 12 cells of the human's centre at 6000 and at least 4 are at 8000.

Not shown: the defence (`ai_raid` is a headless scenario), the finishing `Attack` (not needed in this match: the `AttackMove` fights the centre),
an opponent that fights back.
