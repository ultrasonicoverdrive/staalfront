# WP8.1 screenshots (shroud and fog of war, debug view)

Made by the game itself, under Xvfb with Mesa lavapipe, 1280 x 720, `target/debug/sf_game --debug-fog` (the sandbox scene on the
playfield256 map; `--debug-fog` gives the world vision rules and switches the debug view on, so the "Fog of war (debug)" box of the F1
panel is ticked in the middle picture). The number after `t` in a name is the tick of the snapshot the picture shows. The view is the
**local player's** (player 0, Meridian, the red icons).

* `start_whole_map_t206.png` (`--shots-tick 0`): **the start.** The whole 256 by 256 map. Only the ground round the red base is
  bright: that is what the red side sees (its structures grown by their sight, its units). Everything else is shroud (the dark film).
  The blue base to the right is in the shroud, **and its icons still show through the film**: this is a debug film and not a fair
  view. Hiding what is not seen is WP8.3 and WP8.4.
* `fight_whole_map_t396.png` and `fight_middle_t323.png` (`--fight --shots-tick 200`): **the same with the sides fighting.** The
  bright patch is where the red side has units now; the ground the units crossed on the way is fog (dimmer than shroud, brighter
  than nothing seen). The thin dark dashes right of the blue base are quads of the film sheared by the relief of the map (the
  film is projected onto the terrain run by run); they are a flaw of the debug view, known and left. The middle picture shows the
  F1 panel with the new "Fog of war (debug)" checkbox ticked.

What they do not show: any behaviour. Nothing in the simulation reads the sight yet (WP8.2a); the ticks and cells that matter are
asserted in `tools/sf_headless/tests/vision.rs`.
