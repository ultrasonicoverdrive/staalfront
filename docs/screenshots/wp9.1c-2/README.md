# WP9.1c-2 screenshots (the computer opponent attacks)

The match is `assets/matches/m1_ai.ron` (`sf_game --match m1_ai`): the human (Meridian, south-west) is idle, player 1 (Veyran, north-east) is
`sf_ai` with the profile `normal`, thinking on the game's sim thread. The number after `t` in a name is the tick of the snapshot. Placeholder
models only; blue is Veyran (the AI), red the human.

| File | What it shows |
|---|---|
| `m1_ai_t14000_attack_wave_all_views.png` | **Tick 14000, gathered.** The AI's army (6 units: riflemen, rocket troopers and light tanks, counted by the test) round its gather cell beside its refinery's ore field and its structures. The army floor (2,000) was reached at 13,494; the first wave leaves at tick 14,037 (`max_wait` 14,000, WP9.1c-3). |
| `m1_ai_t15000_attack_wave_all_views.png`, `m1_ai_t15000_whole_map_all_views.png` | **Tick 15000, on the march.** The wave on its `AttackMove` to the human's start site (2 of the 6 within 12 cells of the human's centre, the rest behind); the whole map shows where that is. |
| `m1_ai_t16000_attack_wave_all_views.png` | **Tick 16000, at the human's base.** The 6 units (all within 12 cells of the human's construction centre: the test asserts `>= 4`) beside the human's construction centre (bottom, red), which falls at tick 16,023: the AI wins (`the_army_timing_in_the_game_seat`). |

*Re-taken in WP9.1c-3 (the opening): the WP9.1c-2 pictures were at ticks 6,000, 7,000 and 8,000 with a wave of 12.*

Like the WP9.1d pictures these draw **every** view, seen or not, with the camera on the middle of the AI's army (at 16000 half-way to the human's
centre); in the game the human would see the wave only once it is in the human's sight. Made by `crates/sf_game/tests/ai_screenshot.rs`
(`an_ai_attack_wave_in_the_games_picture`) with `SF_SAVE_SHOTS=docs/screenshots/wp9.1c-2 xvfb-run -a cargo test --release -p sf_game --test
ai_screenshot an_ai_attack` (run from the repository root with an absolute path: the directory is relative to the crate). The test asserts that no AI unit is within 12 cells of the human's centre at 14000 and at least 4 are at 16000.

Not shown: the defence (`ai_raid` is a headless scenario), the finishing `Attack` (not needed in this match: the `AttackMove` fights the centre),
an opponent that fights back.
