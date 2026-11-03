# WP11.5a screenshots (menus and settings)

All from **the real game** on the software adapter (`sf_game ... --shots <dir>` under Xvfb on lavapipe, 1280 x 720, the close-up
shot), except the last one. The tick is in the file name and in the HUD's top bar. The settings files of the runs set only
`ui_scale` and `palette` (`STAALFRONT_SETTINGS=<file>`); the player's own settings file was not read.

* `start_menu_t00001.png`: `sf_game --menu start`. The start menu over the sandbox, which is held at its first tick (1): Match,
  Sandbox, Settings, Quit. The window behind is dimmed.
* `pause_menu_t00100.png`: `sf_game --match --menu pause --shots-tick 100 --focus 60,275`. The pause menu over the match at
  tick 100 (the sim is held there): Resume, Restart, Settings, Main menu, Quit. The sidebar shows the credits (4000), the neutral
  power bar and the power plant on offer.
* `settings_keys_ui100_t00100.png`: the Keys tab at UI scale 100 %: 30 actions in two columns under their headings, each with its
  key from `input.ron` and a Reset button (disabled while the key is the shipped one), the hint "Click a key, then press the new
  one. Esc cancels.", the pager ("Page 1 of 2") and Reset all keys. The tab that is the current page has a ring.
* `settings_keys_ui150_t00100.png`: the same at UI scale 150 %: everything is larger, the page holds fewer rows ("Page 1 of 3").
* `settings_graphics_ui100_t00100.png`: Window (Windowed / Borderless fullscreen), Frame cap (Off, 30, 60, 120, 144 fps) and Vertical
  sync (On / Off); the current choices have a ring.
* `settings_accessibility_ui100_t00100.png`: UI scale 75 to 200 % and Player colours (Standard, Colour-blind safe), each with
  its four colours.
* `settings_accessibility_ui150_colourblind_t00100.png`: the same page at 150 % with the colour-blind safe set chosen: the menu
  is larger, the sidebar behind it too, and the structure at the lower left, which is red in the pictures above, is vermilion.
* `settings_sound_ui100_t00100.png`: the marked empty section "Sound (WP11.5b)": a heading, one line of text and no control.
* `palette_standard_t00045.png` and `palette_colourblind_t00047.png`: `sf_game --sandbox --shots-tick 20 --focus 416,512` without and
  with `palette: ColourBlind`. The team colour of the structures (the red stripes on the cream models, upper left) is the
  standard red in the first and vermilion in the second; the dots of the minimap follow.
* `banner_victory_buttons_t05687.png`: from `crates/sf_game/tests/menu_screenshot.rs` (the UI of a frame through the real renderer
  on a plain background, `SF_SAVE_SHOTS=$PWD/docs/screenshots/wp11.5a`): the Victory banner of a match decided at tick 5687 with
  the two buttons under it, Restart and Main menu. (No match can be won in the window yet, so this one is not the game itself.)

The menus under test are the same functions the HUD calls (`menu_view::begin_frame`, `finish_frame`, `result_buttons`). The
real-window behaviour (Esc, the sim held, nothing reaching the game, Resume, Restart, Main menu, Quit, the key capture, the settings
file) is `scripts/menus_e2e.py`, which prints PASS or FAIL for 26 checks.
