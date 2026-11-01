# WP11.1 screenshots (the sidebar)

Written by `crates/sf_game/tests/sidebar_screenshot.rs` (`SF_SAVE_SHOTS=$PWD/docs/screenshots/wp11.1 cargo test -p
sf_game --test sidebar_screenshot`, an absolute path, since the test runs in the crate directory), real renderer on a
software adapter (lavapipe, under Xvfb), 1280 x 720. The tick is in the name. The harness draws with the functions the
HUD calls (`sidebar::Situation::read`, `sidebar::model`, `sidebar_view::frame`) over the playfield: a construction
centre (84, 74), a power plant, a barracks (84, 80), a vehicle factory (90, 74) and a refinery of player 0 with the
shipped 1000 credits, drawn with the box placeholders of the roster. The sidebar is the column on the right (the minimap,
which the harness does not draw, would sit under it). What the scene does: a second power plant is ordered (300), two
riflemen go into the barracks queue (200) and a light tank into the vehicle factory's (700): 1,200 credits asked of
1,000.

* `sidebar_t00093_structures.png`: the structures tab. Eight structures are on offer (the tech tree has opened: power plant,
  refinery, barracks, vehicle factory, radar, defence tower, shipyard, decoy emplacement; the advanced defences are
  still hidden). The power plant tile (yellow, `PWR`) has the progress bar at 30 percent and a count of 1 in its corner:
  the plant going up. Prices of what the player cannot pay yet (666 credits left) are in the warning colour. The
  power bar is the neutral frame ("Power: no power rules": WP6.4 fills it).
* `sidebar_t00093_infantry.png`: the infantry tab. The rifleman tile (`RIFLE`) is first, at 90 percent with a count
  of 2 (both riflemen in the queue); the others are not queued.
* `sidebar_t00296_vehicles_waiting.png`: the vehicles tab after the credits ran out (Credits 0, in the warning colour):
  the light tank tile (`LTANK`) has an orange bar and says "no credits" next to its price; the two riflemen that came out
  stand beside the barracks. Nothing else on the tab advances.
* `sidebar_t00296_vessels.png`: the vessels tab: no shipyard stands, so there is no tile and the tab says "Needs a
  shipyard".
* `sidebar_t00296_structures.png`: the structures tab again at the later tick.

Not shown: the minimap and the top bar (the harness draws neither); a mouse pointer (the harness has none: the clicks are
tested with `UiInput`, `sidebar_view/tests.rs`).
