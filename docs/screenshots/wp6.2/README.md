# WP6.2 screenshots

`*_tick1.png`: written by `crates/sf_game/tests/placement_screenshot.rs` (`SF_SAVE_SHOTS=<dir> cargo test -p sf_game
--test placement_screenshot`), the real renderer on lavapipe, the sandbox after tick 1, 64 x 64 map, placeholder
shapes. The ghost is drawn by `placement_view::draw_ghost` from `World::preview_placement`.

| File | Shows |
|---|---|
| `valid_meridian_barracks_tick1.png`, `valid_veyran_barracks_tick1.png`, `valid_meridian_decoy_tick1.png`, `valid_veyran_bunker_tick1.png` | allowed places, every cell green, both factions (the barracks has its exit cell in blue) |
| `invalid_terrain_tick1.png` | a power plant half on rock: the rock cells red, the others amber |
| `invalid_structure_tick1.png` | a power plant overlapping a structure: the covered cells red |
| `invalid_too_far_tick1.png` | every cell fine, too far from the base: all amber |
| `wrong_faction_meridian_asks_bunker_tick1.png`, `wrong_faction_veyran_asks_decoy_tick1.png` | the other faction's structure: all amber |
| `factory_exit_tick1.png`, `factory_exit_blocked_tick1.png` | a vehicle factory with its exit cell blue; and with a blocked exit (red exit cell, amber cells) |

`ingame_*.png`: the real `sf_game` binary under Xvfb + lavapipe (`--sandbox --map-cells 64 --build barracks --hover x,y`,
captured with `import -window root`, about tick 170, debug build, so about 12 fps): the ghost, the line under the top bar
with the simulation's reason, the cursor. `ingame_occupied_barracks.png` shows a structure in the way (red cells),
`ingame_exit_blocked_barracks.png` and `ingame_exit_blocked_barracks_2.png` show places refused because the exit is blocked
(amber cells, red exit cell). A place that is fully valid is in the test screenshots above; the in-game runs did not hit one.
