# WP7.1 screenshots: ore on the map

Made by `crates/sf_game/tests/ore_screenshot.rs` (offscreen render, Mesa lavapipe, 640 x 360)
with `SF_SHOT_DIR=docs/screenshots/wp7.1`. The world is made by `playfield::make_world` on
`assets/maps/playfield256` with the shipped `ore.ron` (start 120, maximum 160, +1 every 200
ticks); the picture follows the sim through `OreView` and `Renderer::update_resource_levels`,
as the game does. The file name carries the tick the sim had reached.

| File | Tick | Shows |
|---|---|---|
| `tick-00000-full.png` | 0 | The field around cell (118, 82), 28 cells, every cell at 120 of 160: the full look (dark ochre with bright flecks). |
| `tick-00001-half-depleted.png` | 1 | The west half emptied (bare brown ground, no flecks), the east half taken down to 50 or 80 (few to half the flecks). |
| `tick-16001-regrown.png` | 16001 | 16,000 ticks (13 minutes) later: the emptied cells hold about 80 (the middle level), the east half is full again. |

`*-minimap.png` is the minimap image of the same moment (the whole 256-cell map at one pixel
per cell, the field is the small patch near the top middle; the other ore fields stay full).
