# WP8.3 and WP8.4 screenshots (the look of shroud and fog, ghosts, the minimap)

The pictures named `m1_t*` and `sandbox_t*` come from `crates/sf_game/tests/fog_screenshot.rs` (`SF_SAVE_SHOTS=docs/screenshots/wp8.3`):
the real renderer on Mesa lavapipe, 960 x 540, drawn through the functions the game calls (`SimSnapshot::capture`, `GhostBook::update`,
`runner::scene_motions`, `fog_view::sync_terrain`, `Minimap::set_fog`). The two `game_*` pictures are the game itself, `sf_game --match
--shots <dir> --shots-tick 5` under Xvfb, 1280 x 720. The number after `t` is the tick of the snapshot shown. The view is the local
player's (player 0, Meridian, the red side).

* `m1_t00001_start_base.png`: **the start of the first match.** The construction centre sees ten cells beyond its footprint; the ground
  there is as it always was, then it falls off through a soft edge into black (shroud). The ore field at the right edge of the sight
  is cut off by the shroud, not by a hard line.
* `m1_t00001_start_whole_map.png`: the same start, the whole map: one bright patch, everything else shroud.
* `m1_t00001_fog_edge_close_up.png`: **a fog edge close up**: the east edge of that sight, a ramp of several cells (the test measures
  a row of pixels across it).
* `m1_t00003_scout_sees_the_refinery.png`, `m1_t00900_ghost_of_the_refinery.png`: **the ghost.** A scout of the local player stands
  beside the other side's refinery and defence tower (the first picture shows them as themselves); it walks home, the cells go to fog
  (dimmer, bluish), and the picture at tick 900 shows the defence tower (top) and the refinery (bottom right) as grey-blue ghosts: the
  last view of them. `m1_t01603_ghost_gone.png` is after the refinery was destroyed while nobody looked (the ghost stayed) and the scout
  came back and saw the empty place (the ghost is gone).
* `m1_t00900_minimap_with_fog.png`: the minimap with the same state: shroud black, fog dimmed, the ghost dots, no dot of anything the
  scout cannot see.
* `sandbox_t00003_no_fog.png`: **a world without vision rules** (the same map and structures, no rules): no fog, nothing hidden, the
  scenery markers where they always were. The test compares it with a renderer that never touched the fog: 0 pixels differ.
* `game_m1_t00027_whole_map_real_game.png`, `game_m1_t00011_shroud_only_real_game.png`: **the game itself**, `--match`: the shroud, the
  soft edge round the base, the minimap with fog, the other side's base not shown (neither models nor icons, nor dots on the minimap).
  The second one is the screenshot mode's own close-up camera, which looks at the middle of the map: all shroud.
