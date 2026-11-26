# WP9.4c: the start menu with the AI level

* `start_menu_level_hard_t00001.png`: `sf_game --menu start --shots <dir> --windowed` under Xvfb on lavapipe (1280 x 720, the
  close-up), tick 1, with `STAALFRONT_SETTINGS` naming a settings file that holds `(ai_level: Some("hard"))`. The start menu has
  six buttons now: Match, **Match against the computer**, **Computer level: Hard** (the saved choice; a click cycles
  Easy, Normal, Hard and wraps), Sandbox, Settings, Quit. With no saved choice the button says "Computer level: Normal".
