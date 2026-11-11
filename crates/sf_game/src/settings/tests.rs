use std::path::PathBuf;

use sf_engine::input::{ActionState, InputState, Modifier};
use sf_engine::winit::event::ElementState;

use super::*;

fn shipped() -> Bindings {
    Bindings::builtin()
}

fn key(k: KeyCode) -> Binding {
    Binding::new(Trigger::Key(k))
}

/// A scratch directory of this test, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sf_settings_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| (*v).to_owned())
    }
}

#[test]
fn the_settings_file_is_where_each_system_keeps_its_configuration() {
    let p = |os, pairs: &[(&str, &str)]| settings_path(os, &env(pairs));
    assert_eq!(
        p(Os::Linux, &[("XDG_CONFIG_HOME", "/x/cfg"), ("HOME", "/h")]),
        Some(PathBuf::from("/x/cfg/staalfront/settings.ron"))
    );
    assert_eq!(
        p(Os::Linux, &[("HOME", "/h")]),
        Some(PathBuf::from("/h/.config/staalfront/settings.ron"))
    );
    // An empty XDG variable is not a directory.
    assert_eq!(
        p(Os::Linux, &[("XDG_CONFIG_HOME", ""), ("HOME", "/h")]),
        Some(PathBuf::from("/h/.config/staalfront/settings.ron"))
    );
    assert_eq!(
        p(
            Os::Windows,
            &[("APPDATA", "C:\\Users\\a\\AppData\\Roaming")]
        ),
        Some(PathBuf::from("C:\\Users\\a\\AppData\\Roaming").join("Staalfront/settings.ron"))
    );
    assert_eq!(
        p(Os::MacOs, &[("HOME", "/Users/a")]),
        Some(PathBuf::from(
            "/Users/a/Library/Application Support/Staalfront/settings.ron"
        ))
    );
    assert_eq!(p(Os::Linux, &[]), None);
    assert_eq!(p(Os::Windows, &[("HOME", "/h")]), None);
}

#[test]
fn the_environment_variable_names_the_file_on_every_system() {
    for os in [Os::Linux, Os::Windows, Os::MacOs] {
        let pairs = [
            (ENV_PATH, "/tmp/mine.ron"),
            ("HOME", "/h"),
            ("APPDATA", "/a"),
        ];
        assert_eq!(
            settings_path(os, &env(&pairs)),
            Some(PathBuf::from("/tmp/mine.ron")),
            "{os:?}"
        );
    }
    // Empty means unset.
    let pairs = [(ENV_PATH, ""), ("HOME", "/h")];
    assert_eq!(
        settings_path(Os::Linux, &env(&pairs)),
        Some(PathBuf::from("/h/.config/staalfront/settings.ron"))
    );
}

#[test]
fn the_defaults_are_the_shipped_ones_and_need_no_changes() {
    let d = Settings::default();
    assert_eq!(d.window, WindowMode::Windowed);
    assert_eq!((d.frame_cap, d.vsync, d.ui_scale), (None, true, 1.0));
    assert_eq!(d.palette, PaletteChoice::Standard);
    assert!(d.keys.is_empty());
    let mut again = d.clone();
    assert!(again.sanitize(&shipped()).is_empty());
    assert_eq!(again, d);
}

#[test]
fn a_saved_file_reads_back_the_same_and_the_directory_is_made() {
    let dir = scratch("roundtrip");
    let path = dir.join("deeper/settings.ron");
    let mut s = Settings {
        window: WindowMode::Borderless,
        frame_cap: Some(60),
        vsync: false,
        ui_scale: 1.5,
        palette: PaletteChoice::ColourBlind,
        // Four different volumes and the Red Alert switch (WP11.5b): a field lost or swapped on the way
        // through the file shows.
        sound: SoundSettings {
            master: Some(85),
            music: Some(25),
            effects: Some(55),
            voices: Some(10),
            ra_enabled: Some(true),
            ra_path: Some("/games/ra".into()),
        },
        ..Settings::default()
    };
    s.rebind(&shipped(), Action::OrderStop, KeyCode::KeyV)
        .unwrap();
    save_to(&path, &s).unwrap();
    let loaded = load_from(&path, &shipped());
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(loaded.settings, s);
    // No half-written file is left beside it.
    assert!(!path.with_extension("ron.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_file_is_the_first_run_and_is_not_a_warning() {
    let dir = scratch("missing");
    let loaded = load_from(&dir.join("nothing.ron"), &shipped());
    assert_eq!(loaded.settings, Settings::default());
    assert!(loaded.warnings.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_file_gives_the_defaults_and_one_warning_and_never_a_panic() {
    let dir = scratch("bad");
    let path = dir.join("settings.ron");
    for text in [
        "this is not ron {{{",
        "(window: Sideways)",
        "(ui_scale: \"big\")",
        "",
    ] {
        std::fs::write(&path, text).unwrap();
        let loaded = load_from(&path, &shipped());
        assert_eq!(loaded.settings, Settings::default(), "{text:?}");
        assert_eq!(loaded.warnings.len(), 1, "{text:?}: {:?}", loaded.warnings);
        assert!(loaded.warnings[0].contains("settings.ron"));
    }
    // A directory where the file should be cannot be read either.
    let as_dir = dir.join("is_a_dir.ron");
    std::fs::create_dir_all(&as_dir).unwrap();
    let loaded = load_from(&as_dir, &shipped());
    assert_eq!(loaded.settings, Settings::default());
    assert_eq!(loaded.warnings.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_partial_file_keeps_what_it_says_and_unknown_fields_are_ignored() {
    let dir = scratch("partial");
    let path = dir.join("settings.ron");
    std::fs::write(&path, "(vsync: false, sound: (volume: 7))").unwrap();
    let loaded = load_from(&path, &shipped());
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert!(!loaded.settings.vsync);
    assert_eq!(loaded.settings.ui_scale, 1.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn values_the_game_cannot_use_are_changed_and_said() {
    let dir = scratch("sanitize");
    let path = dir.join("settings.ron");
    std::fs::write(
        &path,
        "(ui_scale: 1.3, frame_cap: Some(5000), keys: {Quit: [(trigger: Key(KeyP))], OrderStop: []})",
    )
    .unwrap();
    let loaded = load_from(&path, &shipped());
    let s = loaded.settings;
    assert_eq!(s.ui_scale, 1.25, "the nearest choice");
    assert_eq!(s.frame_cap, Some(1000));
    assert!(
        s.keys.is_empty(),
        "the menu key and an empty list are dropped"
    );
    assert_eq!(loaded.warnings.len(), 3, "{:?}", loaded.warnings);
    // 0 is no cap, and NaN is the default.
    let mut s = Settings {
        frame_cap: Some(0),
        ui_scale: f32::NAN,
        ..Settings::default()
    };
    assert_eq!(s.sanitize(&shipped()).len(), 1);
    assert_eq!((s.frame_cap, s.ui_scale), (None, 1.0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_write_that_fails_is_said_once_and_does_not_stop_anything() {
    let dir = scratch("unwritable");
    // The target is a directory: the rename cannot replace it.
    let target = dir.join("settings.ron");
    std::fs::create_dir_all(&target).unwrap();
    let mut store = Store::new(Some(target));
    assert!(!store.warned());
    store.save(&Settings::default());
    assert!(store.warned());
    store.save(&Settings::default());
    // Nowhere to write at all is fine too.
    Store::new(None).save(&Settings::default());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rebinding_replaces_the_first_key_and_keeps_the_rest_of_the_action() {
    let base = shipped();
    let mut s = Settings::default();
    // Pan left is A, Left arrow and the left screen edge.
    s.rebind(&base, Action::CameraPanLeft, KeyCode::KeyJ)
        .unwrap();
    let now = s.effective_bindings(&base);
    assert_eq!(
        now.for_action(Action::CameraPanLeft),
        [
            key(KeyCode::KeyJ),
            key(KeyCode::ArrowLeft),
            Binding::new(Trigger::EdgeLeft)
        ]
    );
    // Nothing else changed, and the shipped bindings are what they were.
    for a in Action::ALL
        .into_iter()
        .filter(|a| *a != Action::CameraPanLeft)
    {
        assert_eq!(now.for_action(a), base.for_action(a), "{a:?}");
    }
    assert_eq!(base, Bindings::builtin());
    // Only the changed action is kept in the file.
    assert_eq!(s.keys.len(), 1);
    // With a modifier on the first key, the whole binding is replaced by the plain key; the Alt+Enter
    // of the fullscreen key is the second binding and stays.
    s.rebind(&base, Action::ToggleFullscreen, KeyCode::F9)
        .unwrap();
    let fs = s.effective_bindings(&base);
    assert_eq!(fs.for_action(Action::ToggleFullscreen)[0], key(KeyCode::F9));
    assert_eq!(
        fs.for_action(Action::ToggleFullscreen)[1],
        Binding {
            trigger: Trigger::Key(KeyCode::Enter),
            mods: vec![Modifier::Alt]
        }
    );
}

#[test]
fn a_key_another_action_has_is_refused_and_nothing_changes() {
    let base = shipped();
    let mut s = Settings::default();
    // D is pan right.
    assert_eq!(
        s.rebind(&base, Action::CameraPanLeft, KeyCode::KeyD),
        Err(RebindError::Conflict(Action::CameraPanRight))
    );
    // Enter is bound to the fullscreen key with Alt held: the same key, so the same conflict.
    assert_eq!(
        s.rebind(&base, Action::OrderStop, KeyCode::Enter),
        Err(RebindError::Conflict(Action::ToggleFullscreen))
    );
    // Shift is shared by two actions in the shipped file; nobody else may have it.
    // (The conflict names the first action in the order of `Action`.)
    assert_eq!(
        s.rebind(&base, Action::OrderStop, KeyCode::ShiftLeft),
        Err(RebindError::Conflict(Action::OrderQueue))
    );
    assert_eq!(s, Settings::default());
    // A key another action had before the player moved it is free again, and taken by the mover.
    s.rebind(&base, Action::CameraPanRight, KeyCode::KeyL)
        .unwrap();
    s.rebind(&base, Action::CameraPanLeft, KeyCode::KeyD)
        .unwrap();
    assert_eq!(
        s.rebind(&base, Action::OrderStop, KeyCode::KeyL),
        Err(RebindError::Conflict(Action::CameraPanRight))
    );
}

#[test]
fn escape_and_the_menu_key_cannot_be_rebound() {
    let base = shipped();
    let mut s = Settings::default();
    assert_eq!(
        s.rebind(&base, Action::OrderStop, KeyCode::Escape),
        Err(RebindError::Reserved)
    );
    assert_eq!(
        s.rebind(&base, Action::Quit, KeyCode::KeyP),
        Err(RebindError::NotRebindable)
    );
    assert!(!is_rebindable(Action::Quit));
    assert!(
        Action::ALL
            .into_iter()
            .filter(|a| *a != Action::Quit)
            .all(is_rebindable)
    );
    assert_eq!(s, Settings::default());
}

#[test]
fn giving_an_action_its_shipped_key_back_leaves_nothing_in_the_file() {
    let base = shipped();
    let mut s = Settings::default();
    s.rebind(&base, Action::OrderStop, KeyCode::KeyV).unwrap();
    assert_eq!(s.keys.len(), 1);
    s.rebind(&base, Action::OrderStop, KeyCode::KeyC).unwrap();
    assert!(s.keys.is_empty());
    // Same key twice is no change either.
    s.rebind(&base, Action::OrderStop, KeyCode::KeyC).unwrap();
    assert!(s.keys.is_empty());
    s.rebind(&base, Action::OrderStop, KeyCode::KeyV).unwrap();
    s.rebind(&base, Action::OrderHold, KeyCode::KeyB)
        .unwrap_err();
    s.reset_key(Action::OrderStop);
    assert!(s.keys.is_empty());
    s.rebind(&base, Action::OrderStop, KeyCode::KeyV).unwrap();
    s.rebind(&base, Action::OrderHold, KeyCode::KeyN).unwrap();
    s.reset_all_keys();
    assert!(s.keys.is_empty());
}

/// The rebinding reaches the engine's own action state: the new key fires the action and the old one
/// does not. The engine's code is the code the game runs.
#[test]
fn the_player_s_key_fires_the_action_and_the_old_key_does_not() {
    let base = shipped();
    let mut s = Settings::default();
    s.rebind(&base, Action::OrderStop, KeyCode::KeyV).unwrap();
    let effective = s.effective_bindings(&base);
    let fired = |keycode: KeyCode, b: &Bindings| {
        let mut input = InputState::new();
        input.set_window_size(800, 600);
        input.key_event(keycode, ElementState::Pressed, false);
        let mut state = ActionState::default();
        state.update(&input, b);
        state.pressed(Action::OrderStop)
    };
    assert!(fired(KeyCode::KeyV, &effective));
    assert!(!fired(KeyCode::KeyC, &effective), "the old key is free");
    // And the shipped bindings still do what they did.
    assert!(fired(KeyCode::KeyC, &base));
    assert!(!fired(KeyCode::KeyV, &base));
}

fn opts(preset: crate::args::Preset, line: &str) -> crate::args::Options {
    crate::args::Options::parse(preset, line.split_whitespace().map(str::to_owned))
}

#[test]
fn the_settings_start_the_window_and_a_flag_of_the_command_line_wins() {
    use crate::args::Preset;
    use sf_engine::ScreenMode;
    use sf_render::Present;
    let own = Settings {
        window: WindowMode::Borderless,
        vsync: false,
        frame_cap: Some(60),
        ..Settings::default()
    };
    // Nothing on the command line: the settings decide.
    let s = startup(&opts(Preset::Game, ""), &own);
    assert_eq!(
        s,
        Startup {
            screen: ScreenMode::Borderless,
            present: Present::Uncapped,
            frame_cap: Some(60)
        }
    );
    // The defaults change nothing.
    let s = startup(&opts(Preset::Game, ""), &Settings::default());
    assert_eq!(
        s,
        Startup {
            screen: ScreenMode::Windowed,
            present: Present::Vsync,
            frame_cap: None
        }
    );
    // A flag wins, each over its own setting.
    let s = startup(
        &opts(Preset::Game, "--windowed --present vsync --frame-cap 30"),
        &own,
    );
    assert_eq!(
        s,
        Startup {
            screen: ScreenMode::Windowed,
            present: Present::Vsync,
            frame_cap: Some(30)
        }
    );
    let s = startup(&opts(Preset::Game, "--resolution 800x600"), &own);
    assert_eq!(
        s.screen,
        ScreenMode::Fixed {
            width: 800,
            height: 600
        }
    );
}

/// Measurements do not depend on whoever runs them: the demo and the benchmark ignore the settings.
#[test]
fn the_demo_and_the_benchmark_ignore_the_settings() {
    use crate::args::Preset;
    let own = Settings {
        window: WindowMode::Borderless,
        vsync: false,
        frame_cap: Some(60),
        ..Settings::default()
    };
    for o in [opts(Preset::Demo, ""), opts(Preset::Game, "--bench")] {
        let s = startup(&o, &own);
        assert_eq!(
            (s.screen, s.present, s.frame_cap),
            (o.screen, o.present, o.frame_cap)
        );
    }
}

#[test]
fn which_runs_read_the_users_settings_file() {
    use crate::args::Preset;
    let home = [("HOME", "/h")];
    let with_env = [("HOME", "/h"), (ENV_PATH, "/tmp/s.ron")];
    let user = Some(PathBuf::from("/h/.config/staalfront/settings.ron"));
    let named = Some(PathBuf::from("/tmp/s.ron"));
    let path = |line: &str, preset, pairs: &[(&str, &str)]| {
        path_for(&opts(preset, line), Os::Linux, &env(pairs))
    };
    assert_eq!(path("", Preset::Game, &home), user);
    assert_eq!(path("--match", Preset::Game, &home), user);
    assert_eq!(path("--exit-after 3", Preset::Game, &home), user);
    // The engine demo and the benchmark never touch it, the variable or not.
    assert_eq!(path("", Preset::Demo, &home), None);
    assert_eq!(path("--bench", Preset::Game, &with_env), None);
    // Screenshots only through the variable: not through the home directory.
    assert_eq!(path("--shots out", Preset::Game, &home), None);
    assert_eq!(path("--shots out", Preset::Game, &with_env), named);
    assert_eq!(path("", Preset::Game, &with_env), named);
}

#[test]
fn keys_and_bindings_are_named_the_way_the_player_reads_them() {
    let names = [
        (KeyCode::KeyW, "W"),
        (KeyCode::Digit7, "7"),
        (KeyCode::Numpad3, "Num 3"),
        (KeyCode::ArrowLeft, "Left"),
        (KeyCode::ShiftLeft, "Left Shift"),
        (KeyCode::ControlRight, "Right Ctrl"),
        (KeyCode::Escape, "Esc"),
        (KeyCode::F11, "F11"),
        (KeyCode::PageUp, "Page Up"),
        (KeyCode::Enter, "Enter"),
    ];
    for (k, name) in names {
        assert_eq!(key_name(k), name, "{k:?}");
    }
    assert_eq!(
        binding_name(&Binding {
            trigger: Trigger::Key(KeyCode::Enter),
            mods: vec![Modifier::Alt]
        }),
        "Alt+Enter"
    );
    assert_eq!(
        binding_name(&Binding::new(Trigger::Mouse(
            sf_engine::winit::event::MouseButton::Right
        ))),
        "Right mouse"
    );
    assert_eq!(binding_name(&Binding::new(Trigger::WheelUp)), "Wheel up");
    let b = shipped();
    assert_eq!(
        primary_key_name(&b, Action::CameraPanUp).as_deref(),
        Some("W")
    );
    assert_eq!(primary_key_name(&b, Action::CameraZoomIn), None);
}

#[test]
fn a_hand_edited_file_cannot_give_esc_or_a_key_another_action_has() {
    let dir = scratch("hand_edited_keys");
    let path = dir.join("settings.ron");
    let base = shipped();
    // Escape for the stop order; the key of pan right (D) for pan left; a free key (J) for the
    // fullscreen toggle.
    std::fs::write(
        &path,
        "(keys: {OrderStop: [(trigger: Key(Escape))], CameraPanLeft: [(trigger: Key(KeyD))], \
         ToggleFullscreen: [(trigger: Key(KeyJ))]})",
    )
    .unwrap();
    let loaded = load_from(&path, &shipped());
    let s = &loaded.settings;
    assert!(!s.keys.contains_key(&Action::OrderStop), "Esc is refused");
    assert!(
        !s.keys.contains_key(&Action::CameraPanLeft),
        "D is pan right's"
    );
    assert!(
        s.keys.contains_key(&Action::ToggleFullscreen),
        "a free key stays"
    );
    // The refused actions keep the shipped keys.
    let effective = s.effective_bindings(&base);
    for action in [Action::OrderStop, Action::CameraPanLeft] {
        assert_eq!(effective.for_action(action), base.for_action(action));
    }
    // One warning, naming both and not the third.
    assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
    let w = &loaded.warnings[0];
    assert!(
        w.contains("OrderStop") && w.contains("CameraPanLeft"),
        "{w}"
    );
    assert!(!w.contains("ToggleFullscreen"), "{w}");
    // What the in-game rebind accepts, a file may hold: no warning, nothing dropped.
    let mut by_hand = Settings::default();
    by_hand
        .rebind(&base, Action::CameraPanLeft, KeyCode::KeyJ)
        .unwrap();
    let mut again = by_hand.clone();
    assert!(again.sanitize(&shipped()).is_empty());
    assert_eq!(again, by_hand);
    // Two actions given the same free key: the first in action order keeps it.
    let mut twice = Settings::default();
    twice
        .keys
        .insert(Action::OrderStop, vec![key(KeyCode::KeyJ)]);
    twice
        .keys
        .insert(Action::CameraPanLeft, vec![key(KeyCode::KeyJ)]);
    let first = *twice.keys.keys().next().unwrap();
    let said = twice.sanitize(&shipped());
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(twice.keys.keys().copied().collect::<Vec<_>>(), vec![first]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn esc_is_refused_even_when_the_shipped_file_binds_it_to_nothing() {
    // An `input.ron` where no action has Escape: only the explicit Esc rule refuses it.
    let mut base = shipped();
    for list in base.bindings.values_mut() {
        list.retain(|b| b.trigger != Trigger::Key(KeyCode::Escape));
    }
    assert!(
        base.bindings
            .values()
            .flatten()
            .all(|b| b.trigger != Trigger::Key(KeyCode::Escape))
    );
    let mut s = Settings::default();
    s.keys.insert(Action::OrderStop, vec![key(KeyCode::Escape)]);
    assert_eq!(s.drop_refused_keys(&base), vec![Action::OrderStop]);
    assert!(s.keys.is_empty());
}

/// The `input.ron` the game runs with, not the built-in one, decides what a hand-edited override
/// may use (review N2 of #103): here the file on disk gives the stop order the key J.
fn disk_with_j_for_stop() -> Bindings {
    let mut disk = shipped();
    disk.bindings
        .insert(Action::OrderStop, vec![key(KeyCode::KeyJ)]);
    disk
}

#[test]
fn overrides_are_checked_against_the_input_ron_that_was_loaded() {
    let dir = scratch("sanitize_loaded_input");
    let path = dir.join("settings.ron");
    // J is free in the built-in bindings, but the loaded file has it on the stop order.
    std::fs::write(&path, "(keys: {CameraPanLeft: [(trigger: Key(KeyJ))]})").unwrap();
    let against_builtin = load_from(&path, &shipped());
    assert!(
        against_builtin
            .settings
            .keys
            .contains_key(&Action::CameraPanLeft)
    );
    assert!(against_builtin.warnings.is_empty());
    let disk = disk_with_j_for_stop();
    let loaded = load_from(&path, &disk);
    assert!(loaded.settings.keys.is_empty(), "J is the stop order's");
    assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
    assert!(loaded.warnings[0].contains("CameraPanLeft"));
    // A key the loaded file has freed is allowed: D is not pan right's there.
    let mut freed = shipped();
    freed.bindings.insert(Action::CameraPanRight, Vec::new());
    std::fs::write(&path, "(keys: {CameraPanLeft: [(trigger: Key(KeyD))]})").unwrap();
    assert!(load_from(&path, &shipped()).settings.keys.is_empty());
    assert!(
        load_from(&path, &freed)
            .settings
            .keys
            .contains_key(&Action::CameraPanLeft)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_hot_reloaded_input_ron_is_checked_against_the_overrides_again() {
    let mut s = Settings::default();
    s.rebind(&shipped(), Action::CameraPanLeft, KeyCode::KeyJ)
        .unwrap();
    assert!(s.sanitize_keys(&shipped()).is_empty());
    assert!(s.keys.contains_key(&Action::CameraPanLeft));
    // input.ron is edited: J now belongs to the stop order. The override goes, the shipped keys stay.
    let said = s.sanitize_keys(&disk_with_j_for_stop());
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(s.keys.is_empty());
    let disk = disk_with_j_for_stop();
    assert_eq!(
        s.effective_bindings(&disk)
            .for_action(Action::CameraPanLeft),
        disk.for_action(Action::CameraPanLeft)
    );
}

/// The shipped `audio.ron` numbers.
fn base() -> SoundBase {
    SoundBase::default()
}

#[test]
fn an_old_settings_file_without_sound_keeps_the_numbers_of_audio_ron() {
    let dir = scratch("old_file");
    let path = dir.join("settings.ron");
    // What 11.5a wrote: no `sound` at all.
    std::fs::write(&path, "(window: Borderless, vsync: false, keys: {})").unwrap();
    let loaded = load_from(&path, &shipped());
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(loaded.settings.window, WindowMode::Borderless);
    assert_eq!(loaded.settings.sound, SoundSettings::default());
    let now = loaded.settings.sound.resolve(&base(), None);
    let config = sf_audio::AudioConfig::embedded();
    assert_eq!(
        now.mix,
        Mix {
            master: config.mix.master_volume,
            music: config.mix.music_volume,
            effects: config.mix.sfx_volume,
            voices: config.mix.voice(),
        }
    );
    assert_eq!(now.ra, config.ra_audio);
    // A file with some of the fields: the rest stay on the file's numbers.
    std::fs::write(&path, "(sound: (voices: Some(40)))").unwrap();
    let s = load_from(&path, &shipped()).settings.sound;
    let now = s.resolve(&base(), None);
    assert_eq!(now.mix.voices, 0.4);
    assert_eq!(now.mix.effects, config.mix.sfx_volume);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_volume_beats_audio_ron_and_each_goes_to_its_own_category() {
    let sound = SoundSettings {
        master: Some(90),
        music: Some(70),
        effects: Some(50),
        voices: Some(30),
        ..SoundSettings::default()
    };
    let mix = sound.resolve(&base(), None).mix;
    assert_eq!(
        (mix.master, mix.music, mix.effects, mix.voices),
        (0.9, 0.7, 0.5, 0.3)
    );
    // `audio.ron` of the disk with other numbers is what a field left out falls back to.
    let mut config = sf_audio::AudioConfig::embedded();
    config.mix.music_volume = 0.35;
    let base = SoundBase::from_config(&config);
    let only_effects = SoundSettings {
        effects: Some(20),
        ..SoundSettings::default()
    };
    let mix = only_effects.resolve(&base, None).mix;
    assert_eq!((mix.music, mix.effects), (0.35, 0.2));
}

#[test]
fn the_command_line_folder_wins_over_the_saved_red_alert_setting_for_the_run() {
    let saved = SoundSettings {
        ra_enabled: Some(false),
        ra_path: Some("/saved/ra".into()),
        ..SoundSettings::default()
    };
    let cli = saved.resolve(&base(), Some("/cli/ra")).ra;
    assert!(cli.enabled, "--ra-audio switches it on");
    assert_eq!(cli.path, "/cli/ra");
    // Without the flag: the saved choice, then the file's.
    let own = saved.resolve(&base(), None).ra;
    assert!(!own.enabled);
    assert_eq!(own.path, "/saved/ra");
    let none = SoundSettings::default().resolve(&base(), None).ra;
    assert_eq!(none, sf_audio::AudioConfig::embedded().ra_audio);
    // The flag is not part of the settings: nothing of it is in what would be written.
    assert_eq!(saved.ra_path.as_deref(), Some("/saved/ra"));
}

#[test]
fn a_hand_edited_volume_is_moved_to_a_step_and_said() {
    let dir = scratch("sound_sanitize");
    let path = dir.join("settings.ron");
    std::fs::write(
        &path,
        "(sound: (master: Some(33), music: Some(200), effects: Some(50), voices: Some(0)))",
    )
    .unwrap();
    let loaded = load_from(&path, &shipped());
    let s = loaded.settings.sound;
    assert_eq!(
        (s.master, s.music, s.effects, s.voices),
        (Some(35), Some(100), Some(50), Some(0))
    );
    assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);
    let _ = std::fs::remove_dir_all(&dir);
}
