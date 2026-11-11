# WP11.5b screenshots (the Sound page)

From **the real game** on the software adapter (`sf_game --match --menu sound --shots-tick 100 --focus 60,275 --shots <dir>`
under Xvfb on lavapipe, 1280 x 720, the close-up shot); the tick (100) is in the file names and in the HUD's top bar. Each run
used its own settings file (`STAALFRONT_SETTINGS=<file>`); the player's own file was not read. No audio device exists in the
container, so these show the page, not what it sounds like. The folder `/home/player/redalert` in two of them does not exist: no
Red Alert file was used or is shown.

* `settings_sound_chosen_volumes_t00100.png`: settings file `(sound: (master: Some(85), music: Some(25), effects: Some(55), voices:
  Some(30)))`. Left: Master 85 %, Music 25 %, Effects 55 %, Unit voices 30 % with `-`, `+` and Test (the music has no Test).
  Right: the switch Off (ring), "Folder: none set", the hint. No status line while the switch is off.
* `settings_sound_red_alert_on_not_found_t00100.png`: settings file `(sound: (ra_enabled: Some(true), ra_path:
  Some("/home/player/redalert")))`. The volumes are those of `audio.ron` (100, 60, 100, 100; `+` is dead at 100). The switch is On,
  the folder is shown, and the status line says "Red Alert files not found in /home/player/redalert. The game's own sounds play."
* `settings_sound_cli_folder_t00100.png`: no settings file, `--ra-audio /home/player/redalert` on the command line: the same
  page, so the flag turns the switch on and shows its folder for the run (and writes nothing).

**Re-taken 11-11-2026** (follow-up of #117, same recipes, a debug build): the Music row now keeps the empty place of the Test button, so
its `-`, value and `+` line up with the other three rows; the hint now reads "Start once with --ra-audio FOLDER and click On to remember
it." The pictures show the page only; the interface-group test sound cannot be heard in the container (no audio device).
