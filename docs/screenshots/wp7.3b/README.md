# WP7.3b screenshots: the ore silo

Written by `crates/sf_game/tests/silo_screenshot.rs` with `SF_SAVE_SHOTS=<dir>` (debug build, Mesa
lavapipe in the container). The playfield with one refinery and three harvesters; storage 400 for the
refinery and 200 for the silo (so the store fills in a test's time; the shipped numbers are 2000 and
1500). The silo has no art yet: it is the small box north of the refinery. The storage bar of
`storage_view` is drawn on the UI's world layer; the stored / capacity readout is the F1 debug panel
line (`economy.md`), not in these frames. The tick is in the file name.

| file | what it shows |
|---|---|
| `silo_t01194_refinery_full.png` | Tick 1194: the refinery alone, 400 of 400: its bar in the full colour (red-orange). |
| `silo_t01195_silo_built.png` | Tick 1195: a silo stands north of the refinery; capacity 600, so both bars show 400 of 600 (two thirds, ore colour), the silo's bar as long as its smaller footprint. |
| `silo_t01343_both_full.png` | Tick 1343: the waiting harvester has unloaded into the silo's room; 600 of 600, both bars in the full colour. |
