use sf_ui::UiConfig;

use super::*;
use crate::menu::{Event, Menu, Page, Screen};
use crate::settings::Settings;
use sf_engine::Bindings;

fn look() -> &'static MenuLook {
    MenuLook::embedded()
}

fn menu() -> Menu {
    Menu::new(Settings::default(), Bindings::builtin())
}

fn screen(w: u32, h: u32) -> Rect {
    Rect::new(0.0, 0.0, w as f32, h as f32)
}

/// Text width as the UI font might measure it, near enough for wrapping.
fn measure(scale: f32) -> impl Fn(&str) -> f32 {
    move |t: &str| t.chars().count() as f32 * 7.0 * scale
}

/// Every screen the menus can show.
fn screens() -> Vec<(&'static str, Menu)> {
    let mut out = Vec::new();
    let mut start = menu();
    start.open_start();
    out.push(("start", start));
    let mut pause = menu();
    pause.open_pause();
    out.push(("pause", pause));
    for (name, page) in [
        ("keys", Page::Keys),
        ("graphics", Page::Graphics),
        ("accessibility", Page::Accessibility),
        ("sound", Page::Sound),
    ] {
        let mut m = menu();
        m.open_settings(page);
        out.push((name, m));
    }
    out
}

fn inside(outer: Rect, r: Rect) -> bool {
    r.x >= outer.x - 0.5
        && r.y >= outer.y - 0.5
        && r.right() <= outer.right() + 0.5
        && r.bottom() <= outer.bottom() + 0.5
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.x < b.right() - 0.5
        && b.x < a.right() - 0.5
        && a.y < b.bottom() - 0.5
        && b.y < a.bottom() - 0.5
}

/// Every control rectangle of a layout, with the item it carries.
fn controls(model: &Model, lay: &Layout) -> Vec<(Rect, Item)> {
    let mut out: Vec<(Rect, Item)> = Vec::new();
    out.extend(
        lay.buttons
            .iter()
            .zip(&model.buttons)
            .map(|(r, c)| (*r, c.item)),
    );
    out.extend(lay.tabs.iter().zip(&model.tabs).map(|(r, c)| (*r, c.item)));
    for (placed, row) in lay.rows.iter().zip(&model.rows) {
        if let Row::Setting { controls, .. } = row {
            out.extend(
                placed
                    .controls
                    .iter()
                    .zip(controls)
                    .map(|(r, c)| (*r, c.item)),
            );
        }
    }
    out.extend(
        lay.footer
            .iter()
            .zip(&model.footer)
            .map(|(r, c)| (*r, c.item)),
    );
    out
}

#[test]
fn the_key_list_gets_two_columns_when_the_window_is_wide_and_fewer_rows_when_it_is_short() {
    let l = look();
    let f = fit(screen(1280, 720), 1.0, l);
    assert_eq!(f.columns, 2);
    assert!(f.rows >= 10, "{f:?}");
    // The same window at scale 1.5: still readable, fewer rows.
    let big = fit(screen(1280, 720), 1.5, l);
    assert!(big.rows < f.rows && big.rows >= 3, "{big:?}");
    // A big window shows more rows than a small one.
    assert!(fit(screen(1920, 1080), 1.0, l).rows > f.rows);
    // At scale 2 the panel takes the width of a 1280-wide window: one column.
    assert_eq!(fit(screen(1280, 720), 2.0, l).columns, 1);
    // Never fewer than 3 rows, however small the window.
    assert_eq!(fit(screen(300, 200), 2.0, l).rows, 3);
}

#[test]
fn every_screen_fits_the_window_at_every_scale_the_settings_offer() {
    let l = look();
    for (w, h) in [(1280u32, 720u32), (1920, 1080), (2560, 1440)] {
        for &scale in &l.ui_scales {
            // Scale 2 on a 720 p window is the one case that is allowed not to fit the height; the
            // width must still fit.
            let tall_enough = !(scale >= 2.0 && h < 1080);
            for (name, m) in screens() {
                let f = fit(screen(w, h), scale, l);
                let model = m.model(f).unwrap();
                let lay = layout(&model, l, screen(w, h), scale, &measure(scale));
                let win = screen(w, h);
                let what = format!("{name} at {w}x{h} scale {scale}");
                assert!(
                    lay.panel.x >= 0.0 && lay.panel.right() <= win.right(),
                    "{what}: width"
                );
                if tall_enough {
                    assert!(inside(win, lay.panel), "{what}: {:?}", lay.panel);
                }
                for (r, item) in controls(&model, &lay) {
                    assert!(
                        inside(lay.panel, r),
                        "{what}: {item:?} at {r:?} outside {:?}",
                        lay.panel
                    );
                    assert!(r.w >= 1.0 && r.h >= 1.0, "{what}: {item:?}");
                }
                // No two controls on top of each other.
                let all = controls(&model, &lay);
                for (i, (a, ia)) in all.iter().enumerate() {
                    for (b, ib) in &all[i + 1..] {
                        assert!(!overlap(*a, *b), "{what}: {ia:?} and {ib:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn the_panel_is_centred() {
    let l = look();
    let mut m = menu();
    m.open_pause();
    let w = screen(1280, 720);
    let model = m.model(fit(w, 1.0, l)).unwrap();
    let lay = layout(&model, l, w, 1.0, &measure(1.0));
    assert!((lay.panel.x + lay.panel.w * 0.5 - 640.0).abs() <= 1.0);
    assert!((lay.panel.y + lay.panel.h * 0.5 - 360.0).abs() <= 1.0);
}

#[test]
fn a_click_on_a_control_finds_its_item_and_the_blank_parts_find_none() {
    let l = look();
    let w = screen(1280, 720);
    for (name, m) in screens() {
        let model = m.model(fit(w, 1.0, l)).unwrap();
        let lay = layout(&model, l, w, 1.0, &measure(1.0));
        for (r, item) in controls(&model, &lay) {
            let (x, y) = r.center();
            assert_eq!(hit(&model, &lay, x, y), Some(item), "{name}");
        }
        assert_eq!(
            hit(&model, &lay, 2.0, 2.0),
            None,
            "{name}: outside the panel"
        );
        // The title strip is not a button.
        let (x, y) = lay.title.center();
        assert_eq!(hit(&model, &lay, x, y), None, "{name}: the title");
    }
}

#[test]
fn a_long_line_wraps_at_the_width_and_a_long_word_stays_whole() {
    let m = |t: &str| t.chars().count() as f32 * 10.0;
    assert_eq!(wrap("one two three", 1000.0, &m), ["one two three"]);
    assert_eq!(wrap("one two three", 80.0, &m), ["one two", "three"]);
    assert_eq!(wrap("one two three", 40.0, &m), ["one", "two", "three"]);
    assert_eq!(wrap("extraordinary x", 50.0, &m), ["extraordinary", "x"]);
    assert_eq!(wrap("", 50.0, &m), [""]);
    for line in wrap(&look().words.sound_ra_hint, 400.0, &m) {
        assert!(m(&line) <= 400.0, "{line:?}");
    }
}

/// One frame of the menu on a real `Ui`, the way the HUD does it: `begin_frame`, a game button
/// declared with the input that call returned, `finish_frame`.
struct Frame {
    ui: Ui,
    menu: Menu,
}

impl Frame {
    fn new(menu: Menu) -> Frame {
        Frame {
            ui: Ui::new(UiConfig::default()),
            menu,
        }
    }

    /// Returns the menu button that was clicked, whether the game's button reacted at all, and whether
    /// the UI has the pointer.
    fn run(&mut self, real: UiInput, game_button: Rect) -> (Option<Item>, bool, bool) {
        let open = self.menu.is_open();
        // The game's widgets are declared with the input `begin_frame` hands out.
        let game_input = begin_frame(&mut self.ui, real, (1280, 720), 1.0, open);
        assert_eq!(game_input, if open { UiInput::default() } else { real });
        let game = self.ui.button("game", game_button, "Game", true);
        let menu = open.then_some(&self.menu);
        let clicked = finish_frame(&mut self.ui, real, menu);
        (
            clicked,
            game.clicked || game.hovered || game.pressed,
            self.ui.pointer_over_ui(),
        )
    }
}

fn at(x: f32, y: f32) -> UiInput {
    UiInput {
        cursor: Some((x, y)),
        ..UiInput::default()
    }
}

fn pressed(mut i: UiInput) -> UiInput {
    i.down[0] = true;
    i.pressed[0] = true;
    i
}

fn released(mut i: UiInput) -> UiInput {
    i.released[0] = true;
    i
}

/// A press and a release on a button of the menu, through the same calls the HUD makes, are a click
/// on that button's item: the whole way from a pointer to an `Item`.
#[test]
fn a_press_and_release_on_a_button_gives_its_item() {
    let l = look();
    let w = screen(1280, 720);
    let mut m = menu();
    m.open_pause();
    let model = m.model(fit(w, 1.0, l)).unwrap();
    let lay = layout(&model, l, w, 1.0, &measure(1.0));
    let mut frame = Frame::new(m);
    // Something else at the same place in the game's UI: it must not answer.
    let game_button = lay.buttons[1];
    for (i, item) in [
        Item::Resume,
        Item::Restart,
        Item::OpenSettings,
        Item::MainMenu,
        Item::Quit,
    ]
    .into_iter()
    .enumerate()
    {
        let (x, y) = lay.buttons[i].center();
        frame.run(at(x, y), game_button);
        let (down, _, _) = frame.run(pressed(at(x, y)), game_button);
        assert_eq!(down, None, "a press is not a click");
        let (up, game_reacted, _) = frame.run(released(at(x, y)), game_button);
        assert_eq!(up, Some(item), "button {i}");
        assert!(
            !game_reacted,
            "the game's button under the menu did not react"
        );
    }
}

/// While a menu is open the whole window belongs to the UI: a click beside the panel is not the game's,
/// a game button anywhere is blind; with no menu the same click reaches the game.
#[test]
fn a_menu_takes_the_whole_window_and_the_game_under_it_sees_nothing() {
    let mut m = menu();
    m.open_pause();
    let mut open = Frame::new(m);
    let game_button = Rect::new(20.0, 20.0, 100.0, 30.0);
    // On the game's own button, in the corner far from the panel.
    open.run(at(40.0, 30.0), game_button);
    let (_, reacted, over_ui) = open.run(pressed(at(40.0, 30.0)), game_button);
    assert!(!reacted, "blind: not even hovered");
    assert!(over_ui, "the press is the UI's, wherever it is");
    let (_, reacted, over_ui) = open.run(released(at(40.0, 30.0)), game_button);
    assert!(!reacted && over_ui);
    // Where nothing at all is declared, beside the panel: the window is still the menu's.
    let (_, _, over_ui) = open.run(pressed(at(900.0, 600.0)), game_button);
    assert!(over_ui, "a press beside the panel is not the game's");
    assert!(open.ui.press_consumed(sf_ui::MouseButton::Left));
    open.run(released(at(900.0, 600.0)), game_button);

    let mut closed = Frame::new(menu());
    closed.run(at(40.0, 30.0), game_button);
    closed.run(pressed(at(40.0, 30.0)), game_button);
    let (_, reacted, _) = closed.run(released(at(40.0, 30.0)), game_button);
    assert!(
        reacted,
        "the same click reaches the game when no menu is open"
    );
    let (_, _, over_ui) = closed.run(at(900.0, 600.0), game_button);
    assert!(!over_ui, "and the empty window is the game's");
}

/// The menu draws something, and more of it for the settings than for the start menu.
#[test]
fn a_menu_is_drawn_on_the_dimmed_window() {
    let mut start = menu();
    start.open_start();
    let mut settings = menu();
    settings.open_settings(Page::Keys);
    let quads = |m: Menu| {
        let mut frame = Frame::new(m);
        frame.run(at(5.0, 5.0), Rect::new(900.0, 600.0, 10.0, 10.0));
        frame.ui.draw_list().quad_count()
    };
    let (a, b) = (quads(start), quads(settings));
    println!("quads: start menu {a}, settings (keys) {b}");
    assert!(a > 10, "{a}");
    assert!(b > a, "{b} against {a}");
    // No menu: no menu quads (only the game button's).
    let mut closed = Frame::new(menu());
    closed.run(at(5.0, 5.0), Rect::new(900.0, 600.0, 10.0, 10.0));
    assert!(closed.ui.draw_list().quad_count() < a);
}

/// The buttons under the banner of a decided match: Restart on the left, Main menu on the right,
/// below the banner, and a click on one gives its item.
#[test]
fn the_banner_buttons_stand_under_the_banner_and_answer() {
    let l = look();
    let banner = crate::match_view::shipped_look().banner;
    let bottom = banner.top_px + banner.height_px;
    let click = |x: f32, y: f32| {
        let mut ui = Ui::new(UiConfig::default());
        let run = |ui: &mut Ui, input: UiInput| {
            ui.begin_frame(input, (1280, 720), 1.0);
            let got = result_buttons(ui, l, bottom);
            ui.end_frame();
            got
        };
        run(&mut ui, at(x, y));
        run(&mut ui, pressed(at(x, y)));
        run(&mut ui, released(at(x, y)))
    };
    let r = &l.result_buttons;
    let top = f32::from(bottom + r.below_banner_px);
    let y = top + f32::from(r.height_px) * 0.5;
    let half = f32::from(r.width_px) * 0.5 + f32::from(r.gap_px) * 0.5;
    assert_eq!(click(640.0 - half, y), Some(Item::Restart));
    assert_eq!(click(640.0 + half, y), Some(Item::MainMenu));
    // Above the banner's bottom edge or beside the pair there is nothing.
    assert_eq!(click(640.0, f32::from(bottom) - 5.0), None);
    assert_eq!(click(100.0, y), None);
}

/// The sidebar stays out of the edge-scroll band at every UI scale the accessibility page offers
/// (`sidebar.md`): a cursor is on the sidebar or in the band, never both.
#[test]
fn the_sidebar_keeps_out_of_the_edge_band_at_every_ui_scale_of_the_settings() {
    let look_sidebar = crate::sidebar_view::shipped_look();
    let band = sf_engine::Bindings::builtin().edge_margin as f32;
    for (w, h) in [(1280.0, 720.0), (1920.0, 1080.0)] {
        for &scale in &look().ui_scales {
            let lay = crate::sidebar_layout::layout(
                Rect::new(0.0, 0.0, w, h),
                scale,
                look_sidebar,
                band,
                true,
            );
            let c = lay.column;
            assert!(c.right() <= w - band, "right edge at {w}x{h} scale {scale}");
            assert!(c.y >= band, "top edge at {w}x{h} scale {scale}");
            assert!(c.bottom() <= h, "{w}x{h} scale {scale}");
        }
    }
}

/// What a closed menu costs a frame: the two calls the HUD makes around the game's panels, against the
/// plain `begin_frame` and `end_frame` they replace. A timing, not a test of behaviour: run it with
/// `cargo test --release -p sf_game --lib closed_menu -- --ignored --nocapture` (numbers in `docs/rules/menus.md`).
#[test]
#[ignore = "timing"]
fn a_closed_menu_costs_a_frame_nothing_measurable() {
    use std::time::Instant;
    const FRAMES: u32 = 200_000;
    let real = at(640.0, 360.0);
    let run = |closed_menu_calls: bool| {
        let mut ui = Ui::new(UiConfig::default());
        let started = Instant::now();
        for _ in 0..FRAMES {
            if closed_menu_calls {
                begin_frame(&mut ui, real, (1280, 720), 1.0, false);
                std::hint::black_box(finish_frame(&mut ui, real, None));
            } else {
                ui.begin_frame(real, (1280, 720), 1.0);
                ui.end_frame();
            }
        }
        started.elapsed().as_nanos() as f64 / f64::from(FRAMES)
    };
    // Alternating runs, best of each, so a busy machine shows as noise and not as a difference.
    let (mut plain, mut wrapped) = (f64::MAX, f64::MAX);
    for _ in 0..5 {
        plain = plain.min(run(false));
        wrapped = wrapped.min(run(true));
    }
    println!("per frame: plain {plain:.1} ns, with the menu calls {wrapped:.1} ns");
    assert!(wrapped < plain + 200.0, "{wrapped} against {plain}");
}

/// The tabs stand where they stood when another page is chosen: a click on a tab does not move the
/// next one away from the cursor.
#[test]
fn the_tabs_and_the_title_stay_in_place_from_page_to_page() {
    let l = look();
    for (w, h, scale) in [
        (1280, 720, 1.0),
        (1280, 720, 1.5),
        (1920, 1080, 1.0),
        (1920, 1080, 2.0),
    ] {
        let win = screen(w, h);
        let f = fit(win, scale, l);
        let mut places = Vec::new();
        for page in [Page::Keys, Page::Graphics, Page::Accessibility, Page::Sound] {
            let mut m = menu();
            m.open_settings(page);
            let model = m.model(f).unwrap();
            let lay = layout(&model, l, win, scale, &measure(scale));
            places.push((lay.tabs.clone(), lay.title, lay.panel.x, lay.panel.y));
        }
        for p in &places[1..] {
            assert_eq!(*p, places[0], "{w}x{h} at {scale}");
        }
    }
}

/// The settings screen's layout does not move the pager around: the key list takes the same height on
/// every page of it.
#[test]
fn the_key_list_keeps_its_height_from_page_to_page() {
    let l = look();
    let w = screen(1280, 720);
    let f = Fit {
        rows: 10,
        columns: 2,
    };
    let mut m = menu();
    m.open_settings(Page::Keys);
    let first = layout(&m.model(f).unwrap(), l, w, 1.0, &measure(1.0));
    m.handle(Event::Click(Item::KeysPage(1)));
    assert_eq!(m.screen(), Some(Screen::Settings(Page::Keys)));
    let second = layout(&m.model(f).unwrap(), l, w, 1.0, &measure(1.0));
    assert_eq!(first.panel, second.panel);
    assert_eq!(first.footer.len(), second.footer.len());
}
