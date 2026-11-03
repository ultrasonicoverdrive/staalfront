//! The menus (WP11.5a): the start menu, the pause menu and the settings screen as a state machine that
//! knows nothing of windows, drawing or the simulation. Clicks, the Esc key and a pressed key go in
//! ([`Menu::handle`]); what the game has to do comes out ([`Outcome`]); what to draw is a plain
//! description ([`Menu::model`]) that `menu_view` lays out and the runner never looks into.
//!
//! The rules (what Esc does where, what a refused key says, which settings apply at once) are in
//! `docs/rules/menus.md`; every word and number is `assets/config/menu_look.ron`
//! ([`crate::menu_look`]).

use sf_engine::winit::keyboard::KeyCode;
use sf_engine::{Action, Bindings};

use crate::menu_look::{MenuLook, fill};
use crate::palette::Palette;
use crate::settings::{
    PaletteChoice, RebindError, Settings, WindowMode, binding_name, key_name, primary_key_name,
};

/// A page of the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Keys,
    Graphics,
    Accessibility,
    Sound,
}

impl Page {
    /// The tabs, left to right.
    pub const ALL: [Page; 4] = [Page::Keys, Page::Graphics, Page::Accessibility, Page::Sound];
}

/// A screen of the menu. The menus pile up: the settings lie on the start or the pause menu, and
/// going back uncovers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Start,
    Pause,
    Settings(Page),
}

/// What a click can mean: the identity of every button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// A label that does nothing (a page counter, the fixed menu key).
    Noop,
    StartMatch,
    StartSandbox,
    Resume,
    Restart,
    MainMenu,
    Quit,
    OpenSettings,
    Back,
    Page(Page),
    /// Show this page of the key list.
    KeysPage(usize),
    /// Wait for the new key of this action.
    Rebind(Action),
    ResetKey(Action),
    ResetAllKeys,
    SetWindow(WindowMode),
    SetFrameCap(Option<u32>),
    SetVsync(bool),
    /// The choice with this index in `ui_scales`.
    SetUiScale(usize),
    SetPalette(PaletteChoice),
}

/// What reaches the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Click(Item),
    /// Esc, the menu key.
    Escape,
    /// A key went down (only looked at while a key is awaited).
    Key(KeyCode),
}

/// Which setting changed: the game applies it and the settings are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Window,
    FrameCap,
    Vsync,
    UiScale,
    Palette,
    Keys,
}

/// What the game has to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The menu opened over a running game: hold the simulation.
    Paused,
    /// The menu closed over the game it was holding: let the simulation go on.
    Resumed,
    /// Build the world again from the same setup and play it from the start.
    Restart,
    /// Play the M1 match from the start.
    StartMatch,
    /// Play the sandbox from the start.
    StartSandbox,
    /// The start menu is open over the game; hold the simulation.
    MainMenu,
    Quit,
    Changed(Change),
}

/// What the screen shows of the world outside the menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Live {
    /// The window is in fullscreen now (F11 changes it without the settings).
    pub fullscreen: bool,
}

/// How much of the key list fits the window: rows in a column and columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fit {
    pub rows: usize,
    pub columns: usize,
}

/// A menu to open at the start, for the screenshots (`--menu NAME`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Start,
    Pause,
    Keys,
    Graphics,
    Accessibility,
    Sound,
}

impl Preset {
    /// The name on the command line.
    pub fn parse(name: &str) -> Option<Preset> {
        Some(match name {
            "start" => Preset::Start,
            "pause" => Preset::Pause,
            "keys" => Preset::Keys,
            "graphics" => Preset::Graphics,
            "accessibility" => Preset::Accessibility,
            "sound" => Preset::Sound,
            _ => return None,
        })
    }
}

/// One button of the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub text: String,
    pub item: Item,
    /// The current choice: it gets a ring.
    pub selected: bool,
    pub enabled: bool,
    /// How much of the width of its row it takes, against the other controls of the row.
    pub weight: u8,
    /// Player colours to show beside the text.
    pub swatches: Vec<[u8; 3]>,
}

impl Control {
    fn new(text: impl Into<String>, item: Item) -> Control {
        Control {
            text: text.into(),
            item,
            selected: false,
            enabled: true,
            weight: 1,
            swatches: Vec::new(),
        }
    }

    fn chosen(mut self, selected: bool) -> Control {
        self.selected = selected;
        self
    }

    fn weighted(mut self, weight: u8) -> Control {
        self.weight = weight;
        self
    }

    fn disabled(mut self) -> Control {
        self.enabled = false;
        self
    }
}

/// One line of a settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Heading(String),
    Setting {
        label: String,
        controls: Vec<Control>,
    },
    /// A line of plain text.
    Note(String),
}

/// What kind of screen it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A title and a list of buttons.
    Buttons,
    /// A title, tabs, a page of settings and a footer.
    Settings,
}

/// The line under a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    /// A refusal or a prompt, in the notice colour; else a hint.
    pub warn: bool,
}

/// What a screen shows, ready to be laid out and drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub kind: Kind,
    pub title: String,
    /// The buttons of a [`Kind::Buttons`] screen.
    pub buttons: Vec<Control>,
    /// The tabs of a settings screen.
    pub tabs: Vec<Control>,
    pub rows: Vec<Row>,
    /// Columns the rows are arranged in, top to bottom, left column first.
    pub columns: usize,
    /// The page has room for this many rows in a column whatever they are (so the panel does not
    /// change height as the pages of a long list are turned).
    pub rows_per_column: usize,
    pub notice: Option<Notice>,
    pub footer: Vec<Control>,
}

/// The menu state.
#[derive(Debug)]
pub struct Menu {
    look: &'static MenuLook,
    stack: Vec<Screen>,
    /// The action whose new key is awaited.
    capture: Option<Action>,
    keys_page: usize,
    /// What the last refused key said, until something else happens.
    notice: Option<String>,
    settings: Settings,
    shipped: Bindings,
    live: Live,
}

impl Menu {
    /// A closed menu over `settings`, with `shipped` the bindings of `input.ron`.
    pub fn new(settings: Settings, shipped: Bindings) -> Menu {
        Menu::with_look(MenuLook::embedded(), settings, shipped)
    }

    /// Like [`Menu::new`] with another look (the tests').
    pub fn with_look(look: &'static MenuLook, settings: Settings, shipped: Bindings) -> Menu {
        Menu {
            look,
            stack: Vec::new(),
            capture: None,
            keys_page: 0,
            notice: None,
            settings,
            shipped,
            live: Live::default(),
        }
    }

    pub fn look(&self) -> &'static MenuLook {
        self.look
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The settings read from the file at the start.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// The bindings of `input.ron`, as the game started with them.
    pub fn shipped(&self) -> &Bindings {
        &self.shipped
    }

    /// `input.ron` was read again (hot reload).
    pub fn set_shipped(&mut self, shipped: Bindings) {
        self.shipped = shipped;
    }

    pub fn set_live(&mut self, live: Live) {
        self.live = live;
    }

    /// A menu is open: the game behind it gets no input.
    pub fn is_open(&self) -> bool {
        !self.stack.is_empty()
    }

    /// The screen on top.
    pub fn screen(&self) -> Option<Screen> {
        self.stack.last().copied()
    }

    /// The whole pile, bottom first.
    pub fn stack(&self) -> &[Screen] {
        &self.stack
    }

    /// The action whose new key is awaited, if any: the next key goes to [`Menu::handle`].
    pub fn capturing(&self) -> Option<Action> {
        self.capture
    }

    /// The start menu, over nothing else.
    pub fn open_start(&mut self) {
        self.stack = vec![Screen::Start];
        self.reset_transients();
    }

    /// Esc in the game: the pause menu opens. Does nothing when a menu is open already.
    pub fn open_pause(&mut self) -> Vec<Outcome> {
        if self.is_open() {
            return Vec::new();
        }
        self.stack = vec![Screen::Pause];
        self.reset_transients();
        vec![Outcome::Paused]
    }

    /// Opens a settings page on the pause menu (for screenshots).
    pub fn open_settings(&mut self, page: Page) -> Vec<Outcome> {
        let out = self.open_pause();
        self.stack.push(Screen::Settings(page));
        self.reset_transients();
        out
    }

    /// Opens the menu `preset` names over the game: the pause menu, or a settings page on it; the
    /// start menu is the one that stands under a game that has not begun.
    pub fn open_preset(&mut self, preset: Preset) -> Vec<Outcome> {
        match preset {
            Preset::Start => {
                self.open_start();
                Vec::new()
            }
            Preset::Pause => self.open_pause(),
            Preset::Keys => self.open_settings(Page::Keys),
            Preset::Graphics => self.open_settings(Page::Graphics),
            Preset::Accessibility => self.open_settings(Page::Accessibility),
            Preset::Sound => self.open_settings(Page::Sound),
        }
    }

    fn reset_transients(&mut self) {
        self.capture = None;
        self.keys_page = 0;
        self.notice = None;
    }

    fn close(&mut self) {
        self.stack.clear();
        self.reset_transients();
    }

    /// Handles one event and says what the game has to do.
    pub fn handle(&mut self, event: Event) -> Vec<Outcome> {
        match event {
            Event::Escape => self.escape(),
            Event::Key(key) => self.key(key),
            Event::Click(item) => self.click(item),
        }
    }

    /// Esc: ends the wait for a key; else goes back from the settings; else closes the pause menu. The
    /// start menu has nothing under it, so it stays.
    fn escape(&mut self) -> Vec<Outcome> {
        if self.capture.take().is_some() {
            self.notice = None;
            return Vec::new();
        }
        match self.screen() {
            Some(Screen::Settings(_)) => {
                self.stack.pop();
                self.reset_transients();
                Vec::new()
            }
            Some(Screen::Pause) => {
                self.close();
                vec![Outcome::Resumed]
            }
            Some(Screen::Start) | None => Vec::new(),
        }
    }

    /// A key while one is awaited; any other time it is not the menu's.
    fn key(&mut self, key: KeyCode) -> Vec<Outcome> {
        let Some(action) = self.capture else {
            return Vec::new();
        };
        if key == KeyCode::Escape {
            return self.escape();
        }
        let words = &self.look.words;
        match self.settings.rebind(&self.shipped, action, key) {
            Ok(()) => {
                self.capture = None;
                self.notice = None;
                vec![Outcome::Changed(Change::Keys)]
            }
            Err(RebindError::Conflict(other)) => {
                // The player keeps pressing: the wait goes on.
                self.notice = Some(fill(
                    &words.keys_conflict,
                    &[
                        ("key", &key_name(key)),
                        ("action", &self.action_label(other)),
                    ],
                ));
                Vec::new()
            }
            // Escape never gets here (it ends the wait above) and the menu key is not in the list:
            // nothing to say, the wait is over.
            Err(RebindError::Reserved | RebindError::NotRebindable) => {
                self.capture = None;
                self.notice = None;
                Vec::new()
            }
        }
    }

    fn click(&mut self, item: Item) -> Vec<Outcome> {
        // With no menu open only the buttons under the banner of a decided match are live.
        if !self.is_open() && !matches!(item, Item::Restart | Item::MainMenu) {
            return Vec::new();
        }
        match item {
            Item::Noop => Vec::new(),
            Item::StartMatch => {
                self.close();
                vec![Outcome::StartMatch]
            }
            Item::StartSandbox => {
                self.close();
                vec![Outcome::StartSandbox]
            }
            Item::Resume => {
                self.close();
                vec![Outcome::Resumed]
            }
            Item::Restart => {
                self.close();
                vec![Outcome::Restart]
            }
            Item::MainMenu => {
                self.open_start();
                vec![Outcome::MainMenu]
            }
            Item::Quit => vec![Outcome::Quit],
            Item::OpenSettings => {
                self.stack.push(Screen::Settings(Page::Keys));
                self.reset_transients();
                Vec::new()
            }
            Item::Back => {
                // Back always leaves the page, also while a key is awaited.
                if matches!(self.screen(), Some(Screen::Settings(_))) {
                    self.stack.pop();
                }
                self.reset_transients();
                Vec::new()
            }
            Item::Page(page) => {
                if let Some(top) = self.stack.last_mut()
                    && matches!(top, Screen::Settings(_))
                {
                    *top = Screen::Settings(page);
                }
                self.reset_transients();
                Vec::new()
            }
            Item::KeysPage(n) => {
                self.keys_page = n;
                self.capture = None;
                self.notice = None;
                Vec::new()
            }
            Item::Rebind(action) => {
                self.capture = Some(action);
                self.notice = None;
                Vec::new()
            }
            Item::ResetKey(action) => {
                self.settings.reset_key(action);
                self.capture = None;
                self.notice = None;
                vec![Outcome::Changed(Change::Keys)]
            }
            Item::ResetAllKeys => {
                self.settings.reset_all_keys();
                self.capture = None;
                self.notice = None;
                vec![Outcome::Changed(Change::Keys)]
            }
            Item::SetWindow(mode) => {
                self.settings.window = mode;
                vec![Outcome::Changed(Change::Window)]
            }
            Item::SetFrameCap(cap) => {
                self.settings.frame_cap = cap;
                vec![Outcome::Changed(Change::FrameCap)]
            }
            Item::SetVsync(on) => {
                self.settings.vsync = on;
                vec![Outcome::Changed(Change::Vsync)]
            }
            Item::SetUiScale(i) => match self.look.ui_scales.get(i) {
                Some(&scale) => {
                    self.settings.ui_scale = scale;
                    vec![Outcome::Changed(Change::UiScale)]
                }
                None => Vec::new(),
            },
            Item::SetPalette(choice) => {
                self.settings.palette = choice;
                vec![Outcome::Changed(Change::Palette)]
            }
        }
    }

    /// The label of `action` in the key list, or its name when it is not there.
    fn action_label(&self, action: Action) -> String {
        self.look
            .key_groups
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.action == action)
            .map_or_else(|| format!("{action:?}"), |r| r.label.clone())
    }

    /// What to draw for the screen on top, `None` when no menu is open. `fit` is how much of the key
    /// list the window holds (`menu_view::fit`).
    pub fn model(&self, fit: Fit) -> Option<Model> {
        let words = &self.look.words;
        let model = match self.screen()? {
            Screen::Start => Model::buttons(
                &words.game_title,
                vec![
                    Control::new(&words.start_match, Item::StartMatch),
                    Control::new(&words.start_sandbox, Item::StartSandbox),
                    Control::new(&words.settings, Item::OpenSettings),
                    Control::new(&words.quit, Item::Quit),
                ],
            ),
            Screen::Pause => Model::buttons(
                &words.paused_title,
                vec![
                    Control::new(&words.resume, Item::Resume),
                    Control::new(&words.restart, Item::Restart),
                    Control::new(&words.settings, Item::OpenSettings),
                    Control::new(&words.main_menu, Item::MainMenu),
                    Control::new(&words.quit, Item::Quit),
                ],
            ),
            Screen::Settings(page) => self.settings_model(page, fit),
        };
        Some(model)
    }

    fn settings_model(&self, page: Page, fit: Fit) -> Model {
        let w = &self.look.words;
        let s = &self.settings;
        let tab_text = |p: Page| match p {
            Page::Keys => &w.tab_keys,
            Page::Graphics => &w.tab_graphics,
            Page::Accessibility => &w.tab_accessibility,
            Page::Sound => &w.tab_sound,
        };
        let tabs = Page::ALL
            .into_iter()
            .map(|p| Control::new(tab_text(p), Item::Page(p)).chosen(p == page))
            .collect();
        let back = Control::new(&w.back, Item::Back);
        let mut model = Model {
            kind: Kind::Settings,
            title: w.settings_title.clone(),
            buttons: Vec::new(),
            tabs,
            rows: Vec::new(),
            columns: 1,
            rows_per_column: 0,
            notice: None,
            footer: vec![back],
        };
        match page {
            Page::Keys => self.keys_page_model(&mut model, fit),
            Page::Graphics => {
                let setting = |label: &str, controls: Vec<Control>| Row::Setting {
                    label: label.to_owned(),
                    controls,
                };
                model.rows = vec![
                    setting(
                        &w.window,
                        vec![
                            Control::new(&w.windowed, Item::SetWindow(WindowMode::Windowed))
                                .chosen(!self.live.fullscreen),
                            Control::new(&w.borderless, Item::SetWindow(WindowMode::Borderless))
                                .chosen(self.live.fullscreen),
                        ],
                    ),
                    setting(
                        &w.frame_cap,
                        self.look
                            .frame_caps
                            .iter()
                            .map(|&cap| {
                                let text = cap.map_or_else(
                                    || w.frame_cap_off.clone(),
                                    |hz| fill(&w.frame_cap_hz, &[("hz", &hz.to_string())]),
                                );
                                Control::new(text, Item::SetFrameCap(cap))
                                    .chosen(s.frame_cap == cap)
                            })
                            .collect(),
                    ),
                    setting(
                        &w.vsync,
                        vec![
                            Control::new(&w.on, Item::SetVsync(true)).chosen(s.vsync),
                            Control::new(&w.off, Item::SetVsync(false)).chosen(!s.vsync),
                        ],
                    ),
                ];
            }
            Page::Accessibility => {
                let standard = standard_swatches();
                let blind = Palette::embedded()
                    .players
                    .iter()
                    .map(|c| [c.srgb.0, c.srgb.1, c.srgb.2])
                    .collect::<Vec<_>>();
                let mut standard_button = Control::new(
                    &w.palette_standard,
                    Item::SetPalette(PaletteChoice::Standard),
                )
                .chosen(s.palette == PaletteChoice::Standard);
                standard_button.swatches = standard;
                let mut blind_button = Control::new(
                    &w.palette_colour_blind,
                    Item::SetPalette(PaletteChoice::ColourBlind),
                )
                .chosen(s.palette == PaletteChoice::ColourBlind);
                blind_button.swatches = blind;
                model.rows = vec![
                    Row::Setting {
                        label: w.ui_scale.clone(),
                        controls: self
                            .look
                            .ui_scales
                            .iter()
                            .enumerate()
                            .map(|(i, &scale)| {
                                let percent = (scale * 100.0).round() as i64;
                                Control::new(
                                    fill(&w.ui_scale_percent, &[("percent", &percent.to_string())]),
                                    Item::SetUiScale(i),
                                )
                                .chosen(scale == s.ui_scale)
                            })
                            .collect(),
                    },
                    Row::Setting {
                        label: w.player_colours.clone(),
                        controls: vec![standard_button, blind_button],
                    },
                ];
            }
            Page::Sound => {
                // The place for the sound settings of WP11.5b: marked, and with no controls.
                model.rows = vec![
                    Row::Heading(w.tab_sound.clone()),
                    Row::Note(w.sound_empty.clone()),
                ];
            }
        }
        model
    }

    /// The rows of the key list, all pages.
    fn key_rows(&self) -> Vec<Row> {
        let w = &self.look.words;
        let k = &self.look.keys;
        let effective = self.settings.effective_bindings(&self.shipped);
        let mut rows = Vec::new();
        for group in &self.look.key_groups {
            rows.push(Row::Heading(group.title.clone()));
            for row in &group.rows {
                let waiting = self.capture == Some(row.action);
                let shown = if waiting {
                    w.keys_capture.clone()
                } else {
                    primary_key_name(&effective, row.action).unwrap_or_else(|| {
                        effective
                            .for_action(row.action)
                            .first()
                            .map_or_else(String::new, binding_name)
                    })
                };
                let changed = self.settings.keys.contains_key(&row.action);
                rows.push(Row::Setting {
                    label: row.label.clone(),
                    controls: vec![
                        Control::new(shown, Item::Rebind(row.action))
                            .chosen(waiting)
                            .weighted(k.key_weight),
                        // Nothing to reset while the key is the shipped one.
                        {
                            let mut reset = Control::new(&w.keys_reset, Item::ResetKey(row.action))
                                .weighted(k.reset_weight);
                            reset.enabled = changed;
                            reset
                        },
                    ],
                });
            }
        }
        // The menu key: shown, and fixed.
        rows.push(Row::Setting {
            label: w.keys_menu_row.clone(),
            controls: vec![
                Control::new(&k.fixed_key, Item::Noop)
                    .disabled()
                    .weighted(k.key_weight),
            ],
        });
        rows
    }

    fn keys_page_model(&self, model: &mut Model, fit: Fit) {
        let w = &self.look.words;
        let all = self.key_rows();
        let columns = fit.columns.max(1);
        // A list that fits in one page takes only the room it needs.
        let rows_per_column = fit.rows.min(all.len().div_ceil(columns)).max(1);
        let per_page = rows_per_column * columns;
        let pages = all.len().div_ceil(per_page).max(1);
        let page = self.keys_page.min(pages - 1);
        model.rows = all
            .into_iter()
            .skip(page * per_page)
            .take(per_page)
            .collect();
        model.columns = columns;
        model.rows_per_column = rows_per_column;
        model.notice = Some(match (&self.notice, self.capture) {
            (Some(text), _) => Notice {
                text: text.clone(),
                warn: true,
            },
            (None, Some(action)) => Notice {
                text: fill(
                    &w.keys_capture_notice,
                    &[("action", &self.action_label(action))],
                ),
                warn: true,
            },
            (None, None) => Notice {
                text: w.keys_hint.clone(),
                warn: false,
            },
        });
        let mut footer = Vec::new();
        if pages > 1 {
            footer.push(
                Control::new(&w.keys_prev, Item::KeysPage(page.saturating_sub(1)))
                    .disabled_if(page == 0),
            );
            footer.push(
                Control::new(
                    fill(
                        &w.keys_page,
                        &[
                            ("page", &(page + 1).to_string()),
                            ("pages", &pages.to_string()),
                        ],
                    ),
                    Item::Noop,
                )
                .disabled(),
            );
            footer.push(
                Control::new(&w.keys_next, Item::KeysPage(page + 1)).disabled_if(page + 1 >= pages),
            );
        }
        footer.push(Control::new(&w.keys_reset_all, Item::ResetAllKeys));
        footer.append(&mut model.footer);
        model.footer = footer;
    }
}

impl Control {
    fn disabled_if(mut self, yes: bool) -> Control {
        self.enabled = !yes;
        self
    }
}

impl Model {
    fn buttons(title: &str, buttons: Vec<Control>) -> Model {
        Model {
            kind: Kind::Buttons,
            title: title.to_owned(),
            buttons,
            tabs: Vec::new(),
            rows: Vec::new(),
            columns: 1,
            rows_per_column: 0,
            notice: None,
            footer: Vec::new(),
        }
    }
}

/// The standard player colours (`units.ron`), for the swatches of the palette choice.
pub fn standard_swatches() -> Vec<[u8; 3]> {
    let style = sf_render::Style::embedded();
    (0..Palette::embedded().players.len() as u8)
        .map(|p| style.srgb(p))
        .collect()
}

#[cfg(test)]
mod tests;
