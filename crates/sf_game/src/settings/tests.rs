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
    assert!(again.sanitize().is_empty());
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
        ..Settings::default()
    };
    s.rebind(&shipped(), Action::OrderStop, KeyCode::KeyV)
        .unwrap();
    save_to(&path, &s).unwrap();
    let loaded = load_from(&path);
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(loaded.settings, s);
    // No half-written file is left beside it.
    assert!(!path.with_extension("ron.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_file_is_the_first_run_and_is_not_a_warning() {
    let dir = scratch("missing");
    let loaded = load_from(&dir.join("nothing.ron"));
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
        let loaded = load_from(&path);
        assert_eq!(loaded.settings, Settings::default(), "{text:?}");
        assert_eq!(loaded.warnings.len(), 1, "{text:?}: {:?}", loaded.warnings);
        assert!(loaded.warnings[0].contains("settings.ron"));
    }
    // A directory where the file should be cannot be read either.
    let as_dir = dir.join("is_a_dir.ron");
    std::fs::create_dir_all(&as_dir).unwrap();
    let loaded = load_from(&as_dir);
    assert_eq!(loaded.settings, Settings::default());
    assert_eq!(loaded.warnings.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_partial_file_keeps_what_it_says_and_unknown_fields_are_ignored() {
    let dir = scratch("partial");
    let path = dir.join("settings.ron");
    std::fs::write(&path, "(vsync: false, sound: (volume: 7))").unwrap();
    let loaded = load_from(&path);
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
    let loaded = load_from(&path);
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
    assert_eq!(s.sanitize().len(), 1);
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
