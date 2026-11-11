# WP6.11 screenshots (build speed, sandbox power, silo)

* `game_t00069_sandbox_power.png`: **the real game** (`sf_game --sandbox --harvest --shots-tick 47 --shots <dir>` under Xvfb on
  lavapipe, release build, 1280 x 720, close-up shot at tick 69). The HUD's own power bar reads **"Power: 280/300"**
  (280 used including the refinery `--harvest` adds, 300 made by three power plants), full green, no shortage; before WP6.11 the
  same shot read "Power: 270/100" in the low colour (`docs/screenshots/wp6.4/game_t00065_sandbox.png`). The structures tab
  also lists the ore silo (SILO, 250); the sandbox now places one for both sides (the bar fill of the silo needs ore and is
  shown in `docs/screenshots/wp7.3b/`).

The build speed itself is a number of ticks, not a picture: see the tests named in `docs/rules/production.md` ("Build speed").
