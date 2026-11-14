# WP9.3c: the computer without defences (screenshots)

`m1_duel`, the computer on both start sites, the game's own picture (every view drawn, seen or not), software adapter (Mesa lavapipe) under Xvfb,
the sim thread stepped by hand; the north-east base (camera on cell (72, 17)) and the south-west base (cell (12, 66)). `before` = the
goal list of before WP9.3c written into the profile, `after` = the shipped list. The file names carry the tick. The pictures were made with a
scratch test (not committed; it copied the setup of `ai_screenshot.rs` and replaced the profile's goal list the way `ai_in_game.rs` does).

| File | What it shows |
|---|---|
| `before_m1_duel_t30000_north_east_base_all_views.png` | Tick 30,000, old list: the NE seat owns 13 structures, among them the towers and the radar. |
| `after_m1_duel_t30000_north_east_base_all_views.png` | Tick 30,000, shipped list: 7 structures (centre, two power plants, two refineries, vehicle factory, barracks), no tower, no radar. |
| `*_t15000_*` | Tick 15,000: both lists stand at 7 structures of the NE seat (the old list's first tower is not up yet). |
| `*_south_west_base_*` | The same for the other site (the SW seat's structures are not counted in the numbers above, which are the NE seat's). |

Not shown: the tournament numbers (tables in `docs/rules/ai.md`, "Defence lock (WP9.3c)").
