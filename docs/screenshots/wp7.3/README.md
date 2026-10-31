# WP7.3 screenshots: the ore storage bar

Written by `crates/sf_game/tests/storage_screenshot.rs` with `SF_SAVE_SHOTS=<dir>` (release, Mesa
lavapipe in the container). The playfield with one refinery and three harvesters, storage 400 per
refinery (two loads; the shipped refinery stores 2000), the storage bar of `storage_view` drawn on
the UI's world layer; the cargo bars are not drawn here. The tick is in the file name.

| file | what it shows |
|---|---|
| `storage_t00001_empty.png` | Tick 1: the refinery's bar under its front edge, empty (only the dark track). |
| `storage_t01045_half.png` | Tick 1045: the first load is in, 200 of 400: the bar half filled in the ore colour. |
| `storage_t01213_full.png` | Tick 1213: 400 of 400, the whole bar in the full colour (red-orange); a harvester waits in the bay. |
| `storage_t01214_second_refinery.png` | Tick 1214: a second refinery stands to the north; capacity 800, so both bars show 400 of 800, half, in the ore colour. |
