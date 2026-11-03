# WP6.7 screenshots (defensive buildings)

Written by `crates/sf_game/tests/defences_screenshot.rs` (`SF_SAVE_SHOTS=$PWD/docs/screenshots/wp6.7 xvfb-run -a cargo test -p
sf_game --test defences_screenshot`, an absolute path), real renderer on a software adapter (llvmpipe, under Xvfb), 960 x 540,
the game's own world (units, buildings, power, combat) with the shipped definitions and the glTF models of art set v4. The
number in a name is the number of ticks the world has run. A Meridian (red) structure stands on (30, 30), a Veyran (blue)
light tank near it; the harness draws no sidebar. Each scene is found in the simulation, not placed by hand.

* `tower_t0001_standing.png`, `tower_t0002_firing.png`: **a tower shoots a tank.** The tank stands 5 cells from the tower, the
  tower is powered by a plant out of the picture. At tick 1 the turret still points along the hull; at tick 2, the tick of the
  first shot, it has turned (12 steps a tick) toward the tank, within the arc of its weapon. The picture changes by 731 pixels
  in the tower's place.
* `tower_t0122_no_power_silent.png`: **no power, no shot.** The same tower and tank, no plant: the tower uses 20 power of 0 made,
  the defences are dark. After 120 ticks with the tank in its reach the tower has not fired and its turret is still at rest.
* `tower_t0125_power_back_firing.png`: **the plant stands.** A plant is placed at tick 122: the tower is counted powered, finds
  the tank at its next look, turns and fires (tick 125). 731 pixels differ from the silent picture in the tower's place.
* `emplacement_t0001_turning_0.png`, `emplacement_t0005_turning_1.png`, `emplacement_t0010_turning_2.png`: **an emplacement
  turning.** The advanced defences (turret 8 steps a tick) with a tank to the west, the other way round from its rest facing:
  the turret facings are 8, 40 and 80 steps at ticks 1, 5 and 10, and the pictures differ by 2669 and 2803 pixels.

Not shown: the muzzle flash and tracer (the harness does not run the effects; the event they come from is tested in
`crates/sf_game/tests/fx_events.rs`), and a run of the real game (the sandbox has no scripted attackers; the match needs an
army to reach the opponent's tower at tick 3800).
