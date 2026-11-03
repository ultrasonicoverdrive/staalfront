//! The menus through the real renderer on a software adapter (Mesa lavapipe / DX12 WARP), WP11.5a: the UI
//! of a frame as the HUD builds it (`menu_view::begin_frame`, the game's panels, `menu_view::finish_frame`,
//! `menu_view::result_buttons`) drawn by `sf_ui` on a plain background. The pictures of the whole game
//! with a menu over it are the ones `sf_game --menu NAME --shots DIR` writes (`docs/screenshots/wp11.5a/`).
//!
//! The checks look at the pixels: the window behind a menu is dimmed and the game's own panel with it,
//! the panel and the buttons are in the colours of `ui.ron`, the two buttons stand under the banner of a
//! decided match, and both sets of player colours are drawn in the palette choice. No golden image. Set
//! `SF_SAVE_SHOTS=<dir>` to write the images.

use std::path::PathBuf;

use sf_engine::Bindings;
use sf_game::match_view;
use sf_game::menu::{Item, Menu, Page};
use sf_game::menu_look::MenuLook;
use sf_game::menu_view;
use sf_game::settings::Settings;
use sf_render::image::{self, Rgba8};
use sf_render::{GpuOptions, Renderer};
use sf_sim::{MatchResult, PlayerId, Tick, Verdict};
use sf_ui::{Color, Rect, Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 1280;
const H: u32 = 720;
const BACKGROUND: [u8; 3] = [60, 110, 50];

fn renderer() -> Renderer {
    let mut r = Renderer::headless(W, H, &GpuOptions::software())
        .or_else(|software_err| {
            if std::env::var_os("CI").is_some() {
                return Err(software_err);
            }
            Renderer::headless(
                W,
                H,
                &GpuOptions {
                    backends: sf_render::wgpu::Backends::PRIMARY,
                    ..GpuOptions::default()
                },
            )
        })
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"));
    println!("adapter: {}", r.adapter_summary());
    r.set_scene(&[]);
    r.set_background(BACKGROUND);
    r
}

fn save(name: &str, img: &Rgba8) {
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("shot directory");
        image::write_png(&dir.join(name), img).expect("save shot");
    }
}

fn menu() -> Menu {
    Menu::new(Settings::default(), Bindings::builtin())
}

/// Draws `ui` over the background.
fn draw(r: &mut Renderer, ui: &mut Ui) -> Rgba8 {
    let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
    r.render_to_image_with(|t| ur.render(t, ui))
        .expect("render")
}

/// How far `p` is from `c`, summed over the three colour channels.
fn off(p: [u8; 4], c: [u8; 3]) -> i32 {
    (0..3)
        .map(|i| (i32::from(p[i]) - i32::from(c[i])).abs())
        .sum()
}

fn rgb(c: Color) -> [u8; 3] {
    [c.0, c.1, c.2]
}

/// One frame the way the HUD makes it: a bright panel of "the game" in the corner, then the menu.
fn frame_with_menu(r: &mut Renderer, menu: &Menu, scale_setting: f32) -> (Rgba8, Rect) {
    let mut ui = Ui::new(UiConfig {
        scale: scale_setting,
        ..UiConfig::default()
    });
    let real = UiInput::default();
    menu_view::begin_frame(&mut ui, real, (W, H), 1.0, menu.is_open());
    let game_panel = Rect::new(8.0, 8.0, 140.0, 60.0);
    ui.fill(game_panel, Color::rgb(250, 250, 250));
    menu_view::finish_frame(&mut ui, real, menu.is_open().then_some(menu));
    (draw(r, &mut ui), game_panel)
}

#[test]
fn the_pause_menu_dims_the_window_and_the_game_panel_and_draws_its_buttons() {
    let look = MenuLook::embedded();
    let cfg = UiConfig::default();
    let mut r = renderer();
    // Without a menu: the background and the game's panel as they are.
    let closed = menu();
    let (plain, panel) = frame_with_menu(&mut r, &closed, 1.0);
    assert!(
        off(plain.pixel(900, 600), BACKGROUND) <= 3,
        "the window is the background"
    );
    assert!(off(plain.pixel(20, 20), [250, 250, 250]) <= 3);

    let mut open = menu();
    open.open_pause();
    let (shot, _) = frame_with_menu(&mut r, &open, 1.0);
    save("pause_menu_ui_only.png", &shot);
    // The dim: the colour of the look at its alpha over the background, and over the game's panel too.
    let dim = Color(look.dim.0, look.dim.1, look.dim.2, look.dim.3);
    let dimmed = blend(BACKGROUND, dim);
    assert!(
        off(shot.pixel(900, 600), dimmed) <= 4,
        "{:?} against {dimmed:?}",
        shot.pixel(900, 600)
    );
    let bright = blend([250; 3], dim);
    assert!(
        off(shot.pixel(20, 20), bright) <= 4,
        "the game's panel is dimmed too: {:?}",
        shot.pixel(20, 20)
    );
    assert!(panel.contains(20.0, 20.0));

    // The buttons are where the layout puts them, in the colour of a button.
    let win = Rect::new(0.0, 0.0, W as f32, H as f32);
    let model = open.model(menu_view::fit(win, 1.0, look)).unwrap();
    let lay = menu_view::layout(&model, look, win, 1.0, &|t: &str| t.len() as f32 * 7.0);
    for (i, b) in lay.buttons.iter().enumerate() {
        // A pixel just inside the left edge, clear of the text.
        let p = shot.pixel(b.x as u32 + 8, (b.y + b.h * 0.5) as u32);
        assert!(
            off(p, rgb(cfg.button)) <= BUTTON_TOLERANCE,
            "button {i}: {p:?}"
        );
        // And not the hover colour: nothing here is hovered.
        assert!(
            off(p, rgb(cfg.button_hover)) > 2 * BUTTON_TOLERANCE,
            "button {i}: {p:?}"
        );
    }
    // Inside the panel, in its corner: its dark blue (the skin shades it), not the dimmed window.
    let inside = shot.pixel(lay.panel.x as u32 + 12, lay.panel.y as u32 + 12);
    assert!(off(inside, dimmed) > 30, "{inside:?} is still the window");
    assert!(
        inside[2] > inside[0] + 20 && inside[0] < 70,
        "{inside:?} is not the panel's dark blue (panel {:?})",
        cfg.panel
    );
}

/// The skin of a button shades it a little (a button's pixel is near its colour, not exactly it).
const BUTTON_TOLERANCE: i32 = 32;

/// `over` (straight alpha) drawn over `under` on an sRGB surface: the blend is done in linear light.
fn blend(under: [u8; 3], over: Color) -> [u8; 3] {
    let lin = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let srgb = |l: f32| {
        let v = if l <= 0.0031308 {
            12.92 * l
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (v * 255.0).round() as u8
    };
    let a = f32::from(over.3) / 255.0;
    let mix = |o: u8, u: u8| srgb(lin(o) * a + lin(u) * (1.0 - a));
    [
        mix(over.0, under[0]),
        mix(over.1, under[1]),
        mix(over.2, under[2]),
    ]
}

#[test]
fn a_decided_match_shows_restart_and_main_menu_under_the_banner() {
    let look = MenuLook::embedded();
    let cfg = UiConfig::default();
    let mut r = renderer();
    let mut ui = Ui::new(cfg.clone());
    ui.begin_frame(UiInput::default(), (W, H), 1.0);
    let result = MatchResult {
        tick: Tick(5687),
        verdict: Verdict::Won(PlayerId(0)),
    };
    let drawn = match_view::draw_result(&mut ui, Some(result), PlayerId(0));
    assert!(drawn);
    let banner = match_view::shipped_look().banner;
    let bottom = banner.top_px + banner.height_px;
    menu_view::result_buttons(&mut ui, look, bottom);
    ui.end_frame();
    let shot = draw(&mut r, &mut ui);
    save("banner_victory_buttons_t05687.png", &shot);

    // The banner's word is in its colour at the top; the buttons stand below it, side by side.
    let victory = match_view::shipped_look().victory.border;
    let banner_top = banner.top_px as u32;
    let frame_pixel = shot.pixel(W / 2 - u32::from(banner.width_px) / 2 + 1, banner_top + 40);
    assert!(
        off(frame_pixel, [victory.0, victory.1, victory.2]) <= 8,
        "{frame_pixel:?}"
    );
    let r = &look.result_buttons;
    let top = u32::from(bottom + r.below_banner_px);
    let y = top + u32::from(r.height_px) / 2;
    let half = u32::from(r.width_px) + u32::from(r.gap_px) / 2;
    let (left_x, right_x) = (W / 2 - half + 8, W / 2 + u32::from(r.gap_px) / 2 + 8);
    for (name, x) in [("Restart", left_x), ("Main menu", right_x)] {
        let p = shot.pixel(x, y);
        assert!(off(p, rgb(cfg.button)) <= BUTTON_TOLERANCE, "{name}: {p:?}");
    }
    // Between the two, and above them, the background.
    assert!(
        off(shot.pixel(W / 2, y), BACKGROUND) <= 6,
        "the gap between the buttons"
    );
    assert!(
        off(shot.pixel(W / 2, bottom as u32 + 4), BACKGROUND) <= 6,
        "above the buttons"
    );
}

#[test]
fn the_palette_choice_draws_both_sets_of_player_colours() {
    let look = MenuLook::embedded();
    let mut r = renderer();
    let mut m = menu();
    m.open_settings(Page::Accessibility);
    // Scale 1.5, as a player with a large UI would see it.
    let (shot, _) = frame_with_menu(&mut r, &m, 1.5);
    save("settings_accessibility_scale150_ui_only.png", &shot);
    let win = Rect::new(0.0, 0.0, W as f32, H as f32);
    let f = menu_view::fit(win, 1.5, look);
    let model = m.model(f).unwrap();
    let lay = menu_view::layout(&model, look, win, 1.5, &|t: &str| t.len() as f32 * 7.0);
    // The row with the two palette choices is the last row of the page.
    let row = lay.rows.last().unwrap();
    let standard = sf_game::menu::standard_swatches();
    let blind: Vec<[u8; 3]> = sf_game::palette::Palette::embedded()
        .players
        .iter()
        .map(|c| [c.srgb.0, c.srgb.1, c.srgb.2])
        .collect();
    let count = |area: Rect, c: [u8; 3]| -> usize {
        (area.y as u32..area.bottom() as u32)
            .flat_map(|y| (area.x as u32..area.right() as u32).map(move |x| (x, y)))
            .filter(|&(x, y)| off(shot.pixel(x, y), c) <= 6)
            .count()
    };
    assert_eq!(row.controls.len(), 2);
    for (control, colours, other) in [
        (row.controls[0], &standard, &blind),
        (row.controls[1], &blind, &standard),
    ] {
        for (p, c) in colours.iter().enumerate() {
            assert!(
                count(control, *c) >= 20,
                "player {p} {c:?} is drawn in its choice"
            );
            // The other set's colour of the same player is not in this choice.
            assert_eq!(
                count(control, other[p]),
                0,
                "player {p}: {:?} is the other set's",
                other[p]
            );
        }
    }
    // The choice that is not selected has no ring: the ring colour appears round the selected one only.
    let ring = look.selected.ring;
    let ring = [ring.0, ring.1, ring.2];
    let grown = |c: Rect| Rect::new(c.x - 6.0, c.y - 6.0, c.w + 12.0, c.h + 12.0);
    assert!(
        count(grown(row.controls[0]), ring) > 100,
        "the standard set is the current choice"
    );
    let selected_item = model_item_selected(&model);
    assert_eq!(
        selected_item,
        Some(Item::SetPalette(sf_game::settings::PaletteChoice::Standard))
    );
}

/// The item of the selected palette button of the model.
fn model_item_selected(model: &sf_game::menu::Model) -> Option<Item> {
    match model.rows.last()? {
        sf_game::menu::Row::Setting { controls, .. } => {
            controls.iter().find(|c| c.selected).map(|c| c.item)
        }
        _ => None,
    }
}
