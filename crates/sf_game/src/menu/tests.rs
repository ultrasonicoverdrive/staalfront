use sf_engine::input::Trigger;

use super::*;
use crate::settings::PaletteChoice;

fn menu() -> Menu {
    Menu::new(Settings::default(), Bindings::builtin())
}

fn fit() -> Fit {
    Fit {
        rows: 12,
        columns: 2,
    }
}

fn model(m: &Menu) -> Model {
    m.model(fit()).expect("a menu is open")
}

fn texts(controls: &[Control]) -> Vec<&str> {
    controls.iter().map(|c| c.text.as_str()).collect()
}

fn items(controls: &[Control]) -> Vec<Item> {
    controls.iter().map(|c| c.item).collect()
}

/// The controls of the setting row with this label.
fn row<'a>(model: &'a Model, label: &str) -> &'a [Control] {
    model
        .rows
        .iter()
        .find_map(|r| match r {
            Row::Setting { label: l, controls } if l == label => Some(controls.as_slice()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no row {label:?}"))
}

fn click(m: &mut Menu, item: Item) -> Vec<Outcome> {
    m.handle(Event::Click(item))
}

#[test]
fn the_start_menu_offers_match_sandbox_settings_and_quit_in_that_order() {
    let mut m = menu();
    assert!(!m.is_open() && m.model(fit()).is_none());
    m.open_start();
    let model = model(&m);
    assert_eq!(model.kind, Kind::Buttons);
    assert_eq!(model.title, "Staalfront");
    assert_eq!(
        texts(&model.buttons),
        ["Match", "Sandbox", "Settings", "Quit"]
    );
    assert_eq!(
        items(&model.buttons),
        [
            Item::StartMatch,
            Item::StartSandbox,
            Item::OpenSettings,
            Item::Quit
        ]
    );
}

#[test]
fn the_pause_menu_offers_resume_restart_settings_main_menu_and_quit() {
    let mut m = menu();
    assert_eq!(m.open_pause(), [Outcome::Paused]);
    let model = model(&m);
    assert_eq!(model.title, "Paused");
    assert_eq!(
        texts(&model.buttons),
        ["Resume", "Restart", "Settings", "Main menu", "Quit"]
    );
    assert_eq!(
        items(&model.buttons),
        [
            Item::Resume,
            Item::Restart,
            Item::OpenSettings,
            Item::MainMenu,
            Item::Quit
        ]
    );
    // Opening it again changes nothing, and does not hold the sim a second time.
    assert!(m.open_pause().is_empty());
    assert_eq!(m.stack(), [Screen::Pause]);
}

#[test]
fn esc_closes_the_pause_menu_and_lets_the_game_go_on() {
    let mut m = menu();
    m.open_pause();
    assert_eq!(m.handle(Event::Escape), [Outcome::Resumed]);
    assert!(!m.is_open());
    // With nothing open Esc is not the menu's (the game opens the pause menu).
    assert!(m.handle(Event::Escape).is_empty());
}

#[test]
fn esc_goes_back_one_screen_at_a_time_and_the_start_menu_stays() {
    let mut m = menu();
    m.open_pause();
    click(&mut m, Item::OpenSettings);
    assert_eq!(m.stack(), [Screen::Pause, Screen::Settings(Page::Keys)]);
    // Back from the settings is the pause menu, not the game.
    assert!(m.handle(Event::Escape).is_empty());
    assert_eq!(m.stack(), [Screen::Pause]);
    assert_eq!(m.handle(Event::Escape), [Outcome::Resumed]);

    m.open_start();
    assert!(
        m.handle(Event::Escape).is_empty(),
        "nothing under the start menu"
    );
    assert_eq!(m.stack(), [Screen::Start]);
    click(&mut m, Item::OpenSettings);
    assert!(m.handle(Event::Escape).is_empty());
    assert_eq!(m.stack(), [Screen::Start]);
}

#[test]
fn the_back_button_leaves_the_settings_even_while_a_key_is_awaited() {
    let mut m = menu();
    m.open_pause();
    click(&mut m, Item::OpenSettings);
    click(&mut m, Item::Rebind(Action::OrderStop));
    assert_eq!(m.capturing(), Some(Action::OrderStop));
    click(&mut m, Item::Back);
    assert_eq!(m.stack(), [Screen::Pause]);
    assert_eq!(m.capturing(), None);
}

#[test]
fn every_button_of_the_pause_and_start_menus_does_what_it_says() {
    let outcome = |start: bool, item: Item| {
        let mut m = menu();
        if start {
            m.open_start();
        } else {
            m.open_pause();
        }
        let out = click(&mut m, item);
        (out, m.stack().to_vec())
    };
    assert_eq!(
        outcome(false, Item::Resume),
        (vec![Outcome::Resumed], vec![])
    );
    assert_eq!(
        outcome(false, Item::Restart),
        (vec![Outcome::Restart], vec![])
    );
    assert_eq!(
        outcome(false, Item::MainMenu),
        (vec![Outcome::MainMenu], vec![Screen::Start])
    );
    assert_eq!(
        outcome(false, Item::Quit),
        (vec![Outcome::Quit], vec![Screen::Pause])
    );
    assert_eq!(
        outcome(true, Item::StartMatch),
        (vec![Outcome::StartMatch], vec![])
    );
    assert_eq!(
        outcome(true, Item::StartSandbox),
        (vec![Outcome::StartSandbox], vec![])
    );
    assert_eq!(
        outcome(true, Item::Quit),
        (vec![Outcome::Quit], vec![Screen::Start])
    );
}

/// The buttons under the banner of a decided match work with no menu open; nothing else does.
#[test]
fn with_no_menu_open_only_restart_and_main_menu_answer() {
    let mut m = menu();
    assert_eq!(click(&mut m, Item::Restart), [Outcome::Restart]);
    assert!(!m.is_open());
    assert_eq!(click(&mut m, Item::MainMenu), [Outcome::MainMenu]);
    assert_eq!(m.stack(), [Screen::Start]);
    let mut m = menu();
    for item in [
        Item::Resume,
        Item::Quit,
        Item::StartMatch,
        Item::StartSandbox,
        Item::OpenSettings,
        Item::SetVsync(false),
        Item::Rebind(Action::OrderStop),
        Item::ResetAllKeys,
    ] {
        assert!(click(&mut m, item).is_empty(), "{item:?}");
    }
    assert_eq!(*m.settings(), Settings::default());
    assert!(!m.is_open() && m.capturing().is_none());
}

#[test]
fn the_tabs_switch_the_page_and_the_sound_page_has_its_controls() {
    let mut m = menu();
    m.open_start();
    click(&mut m, Item::OpenSettings);
    let model0 = model(&m);
    assert_eq!(
        texts(&model0.tabs),
        ["Keys", "Graphics", "Accessibility", "Sound"]
    );
    assert_eq!(
        model0.tabs.iter().map(|t| t.selected).collect::<Vec<_>>(),
        [true, false, false, false]
    );
    click(&mut m, Item::Page(Page::Sound));
    assert_eq!(m.screen(), Some(Screen::Settings(Page::Sound)));
    let sound = model(&m);
    assert!(sound.tabs[3].selected);
    assert!(matches!(&sound.rows[0], Row::Heading(h) if h == "Volume"));
    // Four volumes, in this order, each with the shipped audio.ron value; the test button is on all
    // but the music.
    let shown = |label: &str| texts(row(&sound, label)).join(" ");
    assert_eq!(shown("Master"), "- 100% + Test");
    assert_eq!(shown("Music"), "- 60% +");
    // The music row keeps the place of the test button, empty and dead, so it lines up.
    let music = row(&sound, "Music");
    assert_eq!(music.len(), 4);
    assert!(
        music[3].blank && !music[3].enabled && music[3].weight == row(&sound, "Master")[3].weight
    );
    assert_eq!(shown("Effects"), "- 100% + Test");
    assert_eq!(shown("Unit voices"), "- 100% + Test");
    assert!(
        sound
            .rows
            .iter()
            .any(|r| matches!(r, Row::Heading(h) if h == "Original Red Alert audio"))
    );
    assert_eq!(texts(row(&sound, "Use original audio")), ["On", "Off"]);
    assert_eq!(items(&sound.footer), [Item::Back]);
    // A click on a tab on the start menu (no settings open) does nothing to the stack.
    m.handle(Event::Escape);
    assert_eq!(m.stack(), [Screen::Start]);
    click(&mut m, Item::Page(Page::Graphics));
    assert_eq!(m.stack(), [Screen::Start]);
}

/// A menu on the sound page.
fn sound_menu() -> Menu {
    let mut m = menu();
    m.open_settings(Page::Sound);
    m
}

#[test]
fn the_default_of_every_slider_is_the_value_of_audio_ron() {
    let config = sf_audio::AudioConfig::embedded();
    let now = menu().sound_now();
    assert_eq!(now.mix.master, config.mix.master_volume);
    assert_eq!(now.mix.music, config.mix.music_volume);
    assert_eq!(now.mix.effects, config.mix.sfx_volume);
    assert_eq!(now.mix.voices, config.mix.voice());
    // A different file on disk: the page shows that file's numbers until the player chooses.
    let mut other = config.clone();
    other.mix.music_volume = 0.35;
    other.mix.sfx_volume = 0.5;
    other.mix.voice_volume = None;
    let mut m = sound_menu();
    m.set_sound_base(crate::settings::SoundBase::from_config(&other), None);
    let sound = model(&m);
    assert_eq!(texts(row(&sound, "Music"))[1], "35%");
    assert_eq!(texts(row(&sound, "Effects"))[1], "50%");
    assert_eq!(
        texts(row(&sound, "Unit voices"))[1],
        "50%",
        "the voices follow the effects"
    );
}

#[test]
fn a_slider_steps_by_five_and_each_one_sets_its_own_volume() {
    let mut m = sound_menu();
    // Master: 100 -> 95 (the "-" button), music 60 -> 65 ("+"), effects 100 -> 80 in four clicks,
    // voices 100 -> 30: distinct numbers, so a click that reached the wrong slider shows.
    let step = |m: &mut Menu, label: &str, button: usize| {
        let item = row(&model(m), label)[button].item;
        click(m, item)
    };
    assert_eq!(
        step(&mut m, "Master", 0),
        [Outcome::Changed(Change::Volume(Volume::Master))]
    );
    step(&mut m, "Music", 2);
    for _ in 0..4 {
        step(&mut m, "Effects", 0);
    }
    for _ in 0..14 {
        step(&mut m, "Unit voices", 0);
    }
    let s = &m.settings().sound;
    assert_eq!(
        (s.master, s.music, s.effects, s.voices),
        (Some(95), Some(65), Some(80), Some(30))
    );
    let now = m.sound_now();
    assert_eq!(
        (
            now.mix.master,
            now.mix.music,
            now.mix.effects,
            now.mix.voices
        ),
        (0.95, 0.65, 0.8, 0.3)
    );
    let sound = model(&m);
    assert_eq!(
        [
            &texts(row(&sound, "Master"))[1],
            &texts(row(&sound, "Music"))[1],
            &texts(row(&sound, "Effects"))[1],
            &texts(row(&sound, "Unit voices"))[1],
        ],
        [&"95%", &"65%", &"80%", &"30%"]
    );
}

#[test]
fn a_slider_stops_at_nothing_and_at_full_and_a_value_off_the_grid_steps_onto_it() {
    let mut m = sound_menu();
    // Full: "+" is dead. Nothing: "-" is dead and a click would stay at 0.
    let master = row(&model(&m), "Master").to_vec();
    assert!(!master[2].enabled && master[0].enabled);
    for _ in 0..20 {
        let item = row(&model(&m), "Master")[0].item;
        click(&mut m, item);
    }
    assert_eq!(m.settings().sound.master, Some(0));
    let master = row(&model(&m), "Master").to_vec();
    assert!(!master[0].enabled && master[2].enabled);
    // A hand-made 33 % steps to 30 and to 35.
    assert_eq!(
        (step_down(33), step_up(33), step_down(35), step_up(35)),
        (30, 35, 30, 40)
    );
    assert_eq!((step_down(0), step_up(100), step_up(98)), (0, 100, 100));
}

#[test]
fn the_test_button_plays_without_changing_a_setting_and_the_music_has_none() {
    let mut m = sound_menu();
    for which in [Volume::Master, Volume::Effects, Volume::Voices] {
        assert_eq!(
            click(&mut m, Item::TestVolume(which)),
            [Outcome::PlayTest(which)]
        );
    }
    assert_eq!(*m.settings(), Settings::default());
    // The music's place is a blank that answers nothing.
    let music = row(&model(&m), "Music").to_vec();
    assert_eq!(music.len(), 4);
    assert!(music[3].blank);
    assert!(music[..3].iter().all(|c| !c.blank));
}

#[test]
fn the_folder_hint_says_to_start_once_with_the_flag_and_click_on() {
    let m = sound_menu();
    let sound = model(&m);
    let hint = sound
        .rows
        .iter()
        .find_map(|r| match r {
            Row::Note(t) if t.contains("--ra-audio") => Some(t.as_str()),
            _ => None,
        })
        .expect("the hint");
    assert_eq!(
        hint,
        "Your own Red Alert files: nothing from them is copied, only this folder is remembered. \
         Start once with --ra-audio FOLDER and click On to remember it."
    );
}

#[test]
fn the_red_alert_switch_keeps_the_folder_and_the_command_line_gives_way_once_it_is_touched() {
    let mut m = sound_menu();
    // Nothing set: off, no folder, no status line.
    let now = m.sound_now();
    assert!(!now.ra.enabled && now.ra.path.is_empty());
    let notes = |m: &Menu| {
        model(m)
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Note(t) => Some(t.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(notes(&m).len(), 2, "folder and hint only");
    assert_eq!(notes(&m)[0], "Folder: none set");
    // On with no folder: a clear line, not an error.
    assert_eq!(
        click(&mut m, Item::SetRaAudio(true)),
        [Outcome::Changed(Change::RaAudio)]
    );
    assert_eq!(
        notes(&m).last().unwrap(),
        "Red Alert files not found: no folder is set. The game's own sounds play."
    );
    assert_eq!(m.settings().sound.ra_path, None, "no empty path is stored");
    // The folder of --ra-audio: on for this run, shown, and its path is what a click on the switch keeps.
    let mut m = sound_menu();
    m.set_sound_base(
        crate::settings::SoundBase::default(),
        Some("/games/ra".into()),
    );
    assert!(m.sound_now().ra.enabled);
    assert_eq!(notes(&m)[0], "Folder: /games/ra");
    assert_eq!(
        m.settings().sound.ra_enabled,
        None,
        "the flag is not written"
    );
    click(&mut m, Item::SetRaAudio(false));
    assert!(
        !m.sound_now().ra.enabled,
        "the player's click wins over the flag from here on"
    );
    assert_eq!(m.settings().sound.ra_enabled, Some(false));
    assert_eq!(m.settings().sound.ra_path.as_deref(), Some("/games/ra"));
    click(&mut m, Item::SetRaAudio(true));
    assert!(m.sound_now().ra.enabled);
    assert_eq!(m.sound_now().ra.path, "/games/ra");
}

#[test]
fn the_status_line_says_what_became_of_reading_the_red_alert_files() {
    let mut m = sound_menu();
    m.set_sound_base(
        crate::settings::SoundBase::default(),
        Some("/games/ra".into()),
    );
    let last = |m: &Menu| match model(m).rows.last().unwrap() {
        Row::Note(t) => t.clone(),
        other => panic!("{other:?}"),
    };
    for (ra, want) in [
        (RaLoad::Off, "Reading the Red Alert files..."),
        (RaLoad::Loading, "Reading the Red Alert files..."),
        (
            RaLoad::Ready {
                mapped: 0,
                found: 0,
            },
            "Red Alert files not found in /games/ra. The game's own sounds play.",
        ),
        (
            RaLoad::Ready {
                mapped: 7,
                found: 40,
            },
            "Red Alert audio is on: 7 game sounds use it.",
        ),
    ] {
        m.set_live(Live {
            ra,
            ..Live::default()
        });
        assert_eq!(last(&m), want, "{ra:?}");
    }
}

#[test]
fn graphics_choices_change_the_settings_and_say_what_changed() {
    let mut m = menu();
    m.open_pause();
    click(&mut m, Item::OpenSettings);
    click(&mut m, Item::Page(Page::Graphics));
    let g = model(&m);
    assert_eq!(
        texts(row(&g, "Window")),
        ["Windowed", "Borderless fullscreen"]
    );
    assert_eq!(
        texts(row(&g, "Frame cap")),
        ["Off", "30 fps", "60 fps", "120 fps", "144 fps"]
    );
    assert_eq!(texts(row(&g, "Vertical sync")), ["On", "Off"]);
    let chosen = |m: &Model, label: &str| -> Vec<bool> {
        row(m, label).iter().map(|c| c.selected).collect()
    };
    assert_eq!(chosen(&g, "Frame cap"), [true, false, false, false, false]);
    assert_eq!(chosen(&g, "Vertical sync"), [true, false]);

    assert_eq!(
        click(&mut m, Item::SetFrameCap(Some(60))),
        [Outcome::Changed(Change::FrameCap)]
    );
    assert_eq!(
        click(&mut m, Item::SetVsync(false)),
        [Outcome::Changed(Change::Vsync)]
    );
    assert_eq!(
        click(&mut m, Item::SetWindow(WindowMode::Borderless)),
        [Outcome::Changed(Change::Window)]
    );
    let s = m.settings();
    assert_eq!(
        (s.frame_cap, s.vsync, s.window),
        (Some(60), false, WindowMode::Borderless)
    );
    let g = model(&m);
    assert_eq!(chosen(&g, "Frame cap"), [false, false, true, false, false]);
    assert_eq!(chosen(&g, "Vertical sync"), [false, true]);
    // The window row shows the window as it is (F11 changes it without the settings).
    m.set_live(Live {
        fullscreen: true,
        ..Live::default()
    });
    assert_eq!(chosen(&model(&m), "Window"), [false, true]);
    m.set_live(Live {
        fullscreen: false,
        ..Live::default()
    });
    assert_eq!(chosen(&model(&m), "Window"), [true, false]);
}

#[test]
fn the_accessibility_page_sets_the_ui_scale_and_the_player_colours() {
    let mut m = menu();
    m.open_pause();
    click(&mut m, Item::OpenSettings);
    click(&mut m, Item::Page(Page::Accessibility));
    let a = model(&m);
    assert_eq!(
        texts(row(&a, "UI scale")),
        ["75%", "100%", "125%", "150%", "200%"]
    );
    assert_eq!(
        row(&a, "UI scale")
            .iter()
            .map(|c| c.selected)
            .collect::<Vec<_>>(),
        [false, true, false, false, false]
    );
    assert_eq!(
        click(&mut m, Item::SetUiScale(3)),
        [Outcome::Changed(Change::UiScale)]
    );
    assert_eq!(m.settings().ui_scale, 1.5);
    // A choice that is not there changes nothing.
    assert!(click(&mut m, Item::SetUiScale(9)).is_empty());
    assert_eq!(m.settings().ui_scale, 1.5);

    let colours = row(&a, "Player colours");
    assert_eq!(texts(colours), ["Standard", "Colour-blind safe"]);
    assert!(colours[0].selected && !colours[1].selected);
    // Each choice shows its four colours; they are the two sets, not one set twice.
    assert_eq!(colours[0].swatches.len(), 4);
    assert_eq!(colours[1].swatches.len(), 4);
    assert_eq!(colours[0].swatches[0], [214, 40, 40], "units.ron red");
    assert_eq!(
        colours[1].swatches[0],
        [213, 94, 0],
        "the second set's player 0"
    );
    assert_ne!(colours[0].swatches, colours[1].swatches);
    assert_eq!(
        click(&mut m, Item::SetPalette(PaletteChoice::ColourBlind)),
        [Outcome::Changed(Change::Palette)]
    );
    assert_eq!(m.settings().palette, PaletteChoice::ColourBlind);
    let colours = model(&m);
    let colours = row(&colours, "Player colours");
    assert!(!colours[0].selected && colours[1].selected);
}

/// All the rows of the key list, from every page.
fn all_key_rows(m: &mut Menu, fit: Fit) -> Vec<(String, Vec<Control>)> {
    let mut out = Vec::new();
    let mut page = 0;
    loop {
        click(m, Item::KeysPage(page));
        let model = m.model(fit).unwrap();
        for r in &model.rows {
            if let Row::Setting { label, controls } = r {
                out.push((label.clone(), controls.clone()));
            }
        }
        let pages = pages_of(&model);
        page += 1;
        if page >= pages {
            return out;
        }
    }
}

/// How many pages the footer says there are (1 when it says none).
fn pages_of(model: &Model) -> usize {
    model
        .footer
        .iter()
        .find_map(|c| {
            c.text
                .strip_prefix("Page ")?
                .split(" of ")
                .nth(1)?
                .parse()
                .ok()
        })
        .unwrap_or(1)
}

fn keys_menu() -> Menu {
    let mut m = menu();
    m.open_pause();
    click(&mut m, Item::OpenSettings);
    m
}

#[test]
fn the_key_list_shows_the_keys_of_input_ron_and_pages_through_all_of_them() {
    let mut m = keys_menu();
    // Two columns of ten rows: a page of twenty of the 37 lines.
    let ten = Fit {
        rows: 10,
        columns: 2,
    };
    let first = m.model(ten).unwrap();
    assert_eq!(first.columns, 2);
    assert_eq!(first.rows_per_column, 10);
    assert_eq!(first.rows.len(), 20);
    assert_eq!(pages_of(&first), 2);
    let page = first
        .footer
        .iter()
        .find(|c| c.text.starts_with("Page"))
        .unwrap();
    assert_eq!(page.text, "Page 1 of 2");
    let prev = first.footer.iter().find(|c| c.text == "Previous").unwrap();
    let next = first.footer.iter().find(|c| c.text == "Next").unwrap();
    assert!(!prev.enabled && next.enabled);
    assert_eq!(next.item, Item::KeysPage(1));
    click(&mut m, next.item);
    let second = m.model(ten).unwrap();
    let (prev, next) = (
        second.footer.iter().find(|c| c.text == "Previous").unwrap(),
        second.footer.iter().find(|c| c.text == "Next").unwrap(),
    );
    assert!(prev.enabled && !next.enabled);
    assert_eq!(prev.item, Item::KeysPage(0));
    // A page number past the end shows the last page.
    click(&mut m, Item::KeysPage(99));
    assert_eq!(m.model(ten).unwrap().rows, second.rows);

    // Every row of the list appears once, with the key `input.ron` gives it.
    let rows = all_key_rows(&mut m, ten);
    let shipped = Bindings::builtin();
    let look = m.look();
    let listed: Vec<_> = look.key_groups.iter().flat_map(|g| &g.rows).collect();
    assert_eq!(
        rows.len(),
        listed.len() + 1,
        "the rows and the fixed menu key"
    );
    for r in listed {
        let (_, controls) = rows.iter().find(|(l, _)| *l == r.label).unwrap();
        assert_eq!(controls[0].item, Item::Rebind(r.action));
        assert_eq!(
            Some(controls[0].text.clone()),
            crate::settings::primary_key_name(&shipped, r.action),
            "{}",
            r.label
        );
    }
    let (_, menu_row) = rows.iter().find(|(l, _)| l == "Menu / back").unwrap();
    assert_eq!(menu_row[0].text, "Esc");
    assert!(!menu_row[0].enabled && menu_row[0].item == Item::Noop);
    // One column that holds everything is one page, and the pager is gone.
    let tall = Fit {
        rows: 60,
        columns: 1,
    };
    let model = m.model(tall).unwrap();
    assert_eq!(
        model.rows.len(),
        37,
        "5 headings, 31 actions and the menu key"
    );
    assert_eq!(model.rows_per_column, 37, "no more room than it needs");
    assert_eq!(pages_of(&model), 1);
    assert!(model.footer.iter().all(|c| c.item != Item::KeysPage(1)));
}

/// No heading stands alone at the foot of a column, whatever the room, and nothing is lost or
/// reordered by the cutting into pages.
#[test]
fn a_heading_never_ends_a_column_and_the_pages_keep_every_row_in_order() {
    let m = keys_menu();
    let all = m.key_rows();
    let real = |rows: &[Row]| -> Vec<Row> {
        rows.iter()
            .filter(|r| !matches!(r, Row::Spacer))
            .cloned()
            .collect()
    };
    for columns in 1..=3 {
        for per_column in 2..=40 {
            let pages = paginate(&all, per_column, columns);
            let flat: Vec<Row> = pages.iter().flat_map(|p| real(p)).collect();
            assert_eq!(flat, all, "{columns} columns of {per_column}");
            for page in &pages {
                assert!(page.len() <= per_column * columns);
                for column in page.chunks(per_column) {
                    assert!(
                        !matches!(column.last(), Some(Row::Heading(_))),
                        "{columns} columns of {per_column}: a column ends in a heading"
                    );
                }
            }
        }
    }
    // One column of one row cannot avoid it, and must not loop or lose rows.
    let tiny = paginate(&all, 1, 1);
    assert_eq!(tiny.iter().map(|p| real(p).len()).sum::<usize>(), all.len());
    // The list that fits takes one page.
    assert_eq!(paginate(&all, all.len(), 1).len(), 1);
}

#[test]
fn rebinding_waits_for_a_key_takes_it_and_shows_it() {
    let mut m = keys_menu();
    assert_eq!(click(&mut m, Item::Rebind(Action::OrderStop)), []);
    assert_eq!(m.capturing(), Some(Action::OrderStop));
    let waiting = model(&m);
    assert_eq!(
        waiting.notice,
        Some(Notice {
            text: "Press the new key for Stop. Esc cancels.".into(),
            warn: true
        })
    );
    let stop = row(&waiting, "Stop");
    assert_eq!(stop[0].text, "Press a key...");
    assert!(stop[0].selected);

    assert_eq!(
        m.handle(Event::Key(KeyCode::KeyV)),
        [Outcome::Changed(Change::Keys)]
    );
    assert_eq!(m.capturing(), None);
    let done = model(&m);
    assert_eq!(row(&done, "Stop")[0].text, "V");
    assert!(row(&done, "Stop")[1].enabled, "it can be reset now");
    assert!(!row(&done, "Scatter")[1].enabled, "nothing to reset here");
    // The game sees it: the engine's bindings with the player's keys over input.ron.
    let effective = m.settings().effective_bindings(m.shipped());
    assert_eq!(
        effective.for_action(Action::OrderStop)[0].trigger,
        Trigger::Key(KeyCode::KeyV)
    );
    // A key pressed when none is awaited is not the menu's.
    assert!(m.handle(Event::Key(KeyCode::KeyZ)).is_empty());
    assert_eq!(m.settings().keys.len(), 1);
}

#[test]
fn a_key_that_is_taken_is_refused_with_the_name_of_its_action_and_the_wait_goes_on() {
    let mut m = keys_menu();
    click(&mut m, Item::Rebind(Action::OrderStop));
    // D is the pan to the right.
    assert!(m.handle(Event::Key(KeyCode::KeyD)).is_empty());
    assert_eq!(m.capturing(), Some(Action::OrderStop));
    assert_eq!(
        model(&m).notice,
        Some(Notice {
            text: "D is already used for Pan right.".into(),
            warn: true
        })
    );
    assert!(m.settings().keys.is_empty(), "nothing changed");
    // A free key then does it, and the refusal is gone.
    assert_eq!(
        m.handle(Event::Key(KeyCode::KeyV)),
        [Outcome::Changed(Change::Keys)]
    );
    assert!(!model(&m).notice.unwrap().warn);
}

#[test]
fn esc_while_a_key_is_awaited_ends_the_wait_and_nothing_else() {
    let mut m = keys_menu();
    click(&mut m, Item::Rebind(Action::OrderStop));
    assert!(m.handle(Event::Key(KeyCode::Escape)).is_empty());
    assert_eq!(m.capturing(), None);
    assert!(m.settings().keys.is_empty());
    assert_eq!(m.stack(), [Screen::Pause, Screen::Settings(Page::Keys)]);
    // The same through the menu key itself.
    click(&mut m, Item::Rebind(Action::OrderStop));
    assert!(m.handle(Event::Escape).is_empty());
    assert_eq!(m.capturing(), None);
    assert_eq!(m.stack(), [Screen::Pause, Screen::Settings(Page::Keys)]);
    assert_eq!(
        model(&m).notice.unwrap().text,
        "Click a key, then press the new one. Esc cancels."
    );
}

#[test]
fn reset_puts_one_key_or_all_keys_back() {
    let mut m = keys_menu();
    for (action, key) in [
        (Action::OrderStop, KeyCode::KeyV),
        (Action::OrderHold, KeyCode::KeyN),
    ] {
        click(&mut m, Item::Rebind(action));
        m.handle(Event::Key(key));
    }
    assert_eq!(m.settings().keys.len(), 2);
    assert_eq!(
        click(&mut m, Item::ResetKey(Action::OrderStop)),
        [Outcome::Changed(Change::Keys)]
    );
    assert_eq!(m.settings().keys.len(), 1);
    assert_eq!(
        click(&mut m, Item::ResetAllKeys),
        [Outcome::Changed(Change::Keys)]
    );
    assert!(m.settings().keys.is_empty());
    let shown = model(&m);
    assert_eq!(row(&shown, "Stop")[0].text, "C");
}

/// The list names every action of input.ron that a key can be given to, except the menu key: a
/// binding the player cannot reach would be a hidden one.
#[test]
fn no_action_with_a_key_is_missing_from_the_list() {
    let look = MenuLook::embedded();
    let listed: Vec<Action> = look
        .key_groups
        .iter()
        .flat_map(|g| g.rows.iter().map(|r| r.action))
        .collect();
    let shipped = Bindings::builtin();
    for action in Action::ALL {
        let has_key = shipped
            .for_action(action)
            .iter()
            .any(|b| matches!(b.trigger, Trigger::Key(_)));
        let wanted = has_key && crate::settings::is_rebindable(action);
        assert_eq!(listed.contains(&action), wanted, "{action:?}");
    }
    assert_eq!(listed.len(), 31);
}

#[test]
fn open_preset_opens_what_the_screenshots_ask_for() {
    let mut m = menu();
    assert_eq!(m.open_preset(Preset::Keys), [Outcome::Paused]);
    assert_eq!(m.stack(), [Screen::Pause, Screen::Settings(Page::Keys)]);
    let mut m = menu();
    m.open_preset(Preset::Accessibility);
    assert_eq!(m.screen(), Some(Screen::Settings(Page::Accessibility)));
    let mut m = menu();
    assert!(m.open_preset(Preset::Start).is_empty());
    assert_eq!(m.stack(), [Screen::Start]);
    for (name, preset) in [
        ("start", Preset::Start),
        ("pause", Preset::Pause),
        ("keys", Preset::Keys),
        ("graphics", Preset::Graphics),
        ("accessibility", Preset::Accessibility),
        ("sound", Preset::Sound),
    ] {
        assert_eq!(Preset::parse(name), Some(preset));
    }
    assert_eq!(Preset::parse("lobby"), None);
}

/// The menu's `set_shipped` (the hot reload of `input.ron`) checks the player's overrides against the
/// new file and says what it dropped.
#[test]
fn a_reloaded_input_ron_drops_the_overrides_it_now_clashes_with() {
    let mut settings = Settings::default();
    settings
        .rebind(&Bindings::builtin(), Action::CameraPanLeft, KeyCode::KeyJ)
        .unwrap();
    let mut m = Menu::new(settings, Bindings::builtin());
    assert!(m.set_shipped(Bindings::builtin()).is_empty());
    assert!(m.settings().keys.contains_key(&Action::CameraPanLeft));
    let mut edited = Bindings::builtin();
    edited.bindings.insert(
        Action::OrderStop,
        vec![sf_engine::input::Binding::new(Trigger::Key(KeyCode::KeyJ))],
    );
    let said = m.set_shipped(edited.clone());
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(m.settings().keys.is_empty());
    assert_eq!(
        m.shipped().for_action(Action::OrderStop),
        edited.for_action(Action::OrderStop)
    );
}
