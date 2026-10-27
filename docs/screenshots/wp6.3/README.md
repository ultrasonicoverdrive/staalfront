# WP6.3 screenshots: build animation

Written by `crates/sf_game/tests/build_screenshot.rs` (`SF_SAVE_SHOTS=docs/screenshots/wp6.3 cargo test -p
sf_game --test build_screenshot`), through the real renderer on a software adapter (lavapipe), from a
`SimSnapshot` of the world at the tick in the file name (the tick counter after the step). The sandbox's
world with the shipped rules and the art set v4 models; the structure is built with `BuildStructure` next to
the base, so the tick is that of the world, not of the build (the first tick of the build is the one before).

| File | Shows |
|---|---|
| `meridian_power_plant_foundation_tick32` | Meridian power plant 10 % done (`build_ticks` 300): the foundation slab and the bar a tenth full |
| `meridian_power_plant_scaffolding_tick212` | the same at 70 %: scaffolding (the stage from 400 thousandths) and the bar two thirds full |
| `meridian_power_plant_finished_tick302` | the tick after it completed: the finished model, no bar |
| `veyran_power_plant_foundation_tick333`, `..._scaffolding_tick513`, `..._finished_tick603` | the same three stages for Veyran (blue) |
| `meridian_decoy_emplacement_foundation_tick18`, `..._finished_tick162` | the decoy (160 ticks) |
| `veyran_fortified_bunker_foundation_tick213`, `..._finished_tick663` | the Veyran bunker (500 ticks) |

The other structures in each picture are the sandbox's, placed complete with `PlaceStructure`: they have no bar and
are drawn finished.
