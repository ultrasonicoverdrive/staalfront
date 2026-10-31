# WP7.2 screenshots: harvesters

Two sets, both through the real renderer on Mesa lavapipe (software Vulkan).

## From the game (`sf_game`, Xvfb, real models)

```
xvfb-run -a -s "-screen 0 1280x720x24" target/debug/sf_game --map assets/maps/playfield256 --harvest \
    --focus <x,z metres> --shots <dir> --shots-tick <N>
```

The `close_up` shot of each run, named with the tick it was taken at (the HUD line shows it too).
`--harvest` puts a refinery on (106, 82) and three harvesters of Meridian beside the ore field round (120, 84).

| File | Tick | Shows |
|---|---|---|
| `game_t0311_mining.png` | 311 | A harvester mining at the edge of the field: the cargo bar under it is part filled, the ore under it has gone from the full look to the bare, dark look. |
| `game_t0591_driving_back_full.png` | 591 | Two harvesters driving back to the refinery with full loads (full bars, the brighter fill), the drained ground behind them. |
| `game_t0729_docked_and_queued.png` | 729 | The refinery on the left: one harvester at the dock point with an empty bar (it is being or has just been unloaded), the next one standing behind it with a full bar: the queue. |

## From the screenshot test (`crates/sf_game/tests/harvest_screenshot.rs`, shapes instead of models)

Written with `SF_SAVE_SHOTS=<dir>`; the tick is in the name, and the test found each moment in
the simulation (`World::harvester_of`, `World::dock_of`), it did not pick ticks by hand.

| File | Shows |
|---|---|
| `harvest_t*_start.png` | Before the first bite: empty tracks, no fill. |
| `harvest_t*_mining.png` | Mining: the bars are part filled. |
| `harvest_t*_full_load.png` | A full load: the brighter fill. |
| `harvest_t*_driving_back.png` | The first harvester driving back. |
| `harvest_t*_queueing.png` | One in the bay and one queued at the refinery. |
| `harvest_t*_unloading.png` | The harvester in the bay with its load part way down. |
