//! Laying out and drawing the menus (WP11.5a). [`layout`] is a pure function of a [`Model`], the window,
//! the UI scale and `menu_look.ron`, so a test can lay a menu out at any size and click on it;
//! [`frame`] draws it on an `sf_ui::Ui` and reports the button that was clicked. The window behind a
//! menu is dimmed and takes the pointer ([`sf_ui::Ui::block_rect`]), so no click beside the panel
//! is the game's.

use sf_ui::{Anchor, Color, Rect, Ui, UiInput};

use crate::menu::{Fit, Item, Kind, Menu, Model, Row};
use crate::menu_look::MenuLook;

fn color(c: (u8, u8, u8, u8)) -> Color {
    Color(c.0, c.1, c.2, c.3)
}

fn px(v: u16, scale: f32) -> f32 {
    (f32::from(v) * scale).round()
}

/// The placed parts of a settings row.
#[derive(Clone, Debug, PartialEq)]
pub struct RowLayout {
    /// The whole line.
    pub rect: Rect,
    /// The label's box (empty for a heading or a note).
    pub label: Rect,
    /// One rectangle for each control of the row, in order.
    pub controls: Vec<Rect>,
    /// The lines of a note, wrapped to the width of the row.
    pub lines: Vec<String>,
}

/// Where everything of a menu is, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub panel: Rect,
    /// The strip the title is centred in.
    pub title: Rect,
    /// The buttons of a [`Kind::Buttons`] menu.
    pub buttons: Vec<Rect>,
    pub tabs: Vec<Rect>,
    pub rows: Vec<RowLayout>,
    pub notice: Option<Rect>,
    pub footer: Vec<Rect>,
}

/// How many columns and rows of the key list the window holds at `scale`: the columns the panel is
/// wide enough for, and the rows that fit in the height left by everything else.
pub fn fit(screen: Rect, scale: f32, look: &MenuLook) -> Fit {
    let p = &look.panel;
    // The key list is on the settings screen: the wide panel.
    let panel_w = panel_width(screen, scale, look, false);
    let inner_w = panel_w - 2.0 * px(p.pad_px, scale);
    let gap = px(p.gap_px, scale);
    let min_col = px(look.keys.column_min_px, scale);
    let columns = (1..=usize::from(look.keys.columns))
        .rev()
        .find(|&c| inner_w >= c as f32 * min_col + (c as f32 - 1.0) * gap)
        .unwrap_or(1);
    let chrome = 2.0 * px(p.pad_px, scale)
        + title_height(look, scale)
        + px(look.tab.height_px, scale)
        + px(p.notice_height_px, scale)
        + px(look.footer.height_px, scale)
        + 4.0 * gap;
    let room = screen.h - 2.0 * px(p.margin_px, scale) - chrome;
    let pitch = px(look.row.height_px, scale) + px(look.row.gap_px, scale);
    let rows = if room > 0.0 && pitch > 0.0 {
        (room / pitch).floor() as usize
    } else {
        0
    };
    Fit {
        rows: rows.max(3),
        columns,
    }
}

/// The width of the panel: the settings' own, or the narrow one of a list of buttons, and never more
/// than the window less its margins.
fn panel_width(screen: Rect, scale: f32, look: &MenuLook, compact: bool) -> f32 {
    let p = &look.panel;
    let want = if compact {
        p.compact_width_px
    } else {
        p.width_px
    };
    px(want, scale).min((screen.w - 2.0 * px(p.margin_px, scale)).max(0.0))
}

fn title_height(look: &MenuLook, scale: f32) -> f32 {
    (f32::from(look.panel.title_px) * 1.3 * scale).round()
}

/// Splits `text` into lines no wider than `max_w` by `measure`; a word wider than a line stays whole.
pub fn wrap(text: &str, max_w: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if measure(&format!("{line} {word}")) <= max_w => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Splits `width` among `weights` with `gap` between: widths that add up to `width` less the gaps,
/// each at least 1 px.
fn split(width: f32, weights: &[u8], gap: f32) -> Vec<f32> {
    let n = weights.len().max(1);
    let total: f32 = weights.iter().map(|&w| f32::from(w)).sum::<f32>().max(1.0);
    let room = (width - gap * (n as f32 - 1.0)).max(n as f32);
    weights
        .iter()
        .map(|&w| (room * f32::from(w) / total).floor().max(1.0))
        .collect()
}

/// Lays `model` out in `screen` (physical pixels) at UI `scale`. `measure` gives the width in
/// physical pixels of a line of text at the default size; it decides where a note wraps.
pub fn layout(
    model: &Model,
    look: &MenuLook,
    screen: Rect,
    scale: f32,
    measure: &dyn Fn(&str) -> f32,
) -> Layout {
    let p = &look.panel;
    let (pad, gap) = (px(p.pad_px, scale), px(p.gap_px, scale));
    let panel_w = panel_width(screen, scale, look, model.kind == Kind::Buttons);
    let inner_w = (panel_w - 2.0 * pad).max(0.0);
    let title_h = title_height(look, scale);
    let row_h = px(look.row.height_px, scale);
    let row_gap = px(look.row.gap_px, scale);
    let pitch = row_h + row_gap;

    // Everything is first laid out from the panel's top left; the panel is centred after.
    let x0 = pad;
    let mut y = pad;
    let title = Rect::new(0.0, y, panel_w, title_h);
    y += title_h + gap;

    let mut buttons = Vec::new();
    let mut tabs = Vec::new();
    let mut rows = Vec::new();
    let mut notice = None;
    let mut footer = Vec::new();
    // The height this screen has less than the tallest settings page.
    let mut slack = 0.0f32;

    match model.kind {
        Kind::Buttons => {
            let w = px(look.button.width_px, scale).min(inner_w);
            let h = px(look.button.height_px, scale);
            for _ in &model.buttons {
                buttons.push(Rect::new(x0 + (inner_w - w) * 0.5, y, w, h));
                y += h + gap;
            }
        }
        Kind::Settings => {
            let tab_h = px(look.tab.height_px, scale);
            let n = model.tabs.len().max(1);
            let widths = split(inner_w, &vec![1; n], gap);
            let mut x = x0;
            for w in &widths {
                tabs.push(Rect::new(x, y, *w, tab_h));
                x += w + gap;
            }
            y += tab_h + gap;

            let columns = model.columns.max(1);
            let col_w = ((inner_w - gap * (columns as f32 - 1.0)) / columns as f32).floor();
            let per_column = if model.rows_per_column > 0 {
                model.rows_per_column
            } else {
                model.rows.len().max(1)
            };
            let mut used = vec![0.0f32; columns];
            for (i, row) in model.rows.iter().enumerate() {
                let col = (i / per_column).min(columns - 1);
                let top = y + used[col];
                let left = x0 + col as f32 * (col_w + gap);
                let placed = match row {
                    Row::Heading(_) | Row::Spacer => {
                        let rect = Rect::new(left, top, col_w, row_h);
                        used[col] += pitch;
                        RowLayout {
                            rect,
                            label: rect,
                            controls: Vec::new(),
                            lines: Vec::new(),
                        }
                    }
                    Row::Note(text) => {
                        let lines = wrap(text, col_w, measure);
                        let h = (row_h * 0.8).round() * lines.len() as f32;
                        let rect = Rect::new(left, top, col_w, h);
                        used[col] += h + row_gap;
                        RowLayout {
                            rect,
                            label: rect,
                            controls: Vec::new(),
                            lines,
                        }
                    }
                    Row::Setting { controls, .. } => {
                        let rect = Rect::new(left, top, col_w, row_h);
                        used[col] += pitch;
                        let label_w = px(look.row.label_px, scale).min((col_w * 0.5).floor());
                        let label = Rect::new(left, top, label_w, row_h);
                        let weights: Vec<u8> = controls.iter().map(|c| c.weight).collect();
                        let space = (col_w - label_w - row_gap).max(1.0);
                        let widths = split(space, &weights, row_gap);
                        let mut x = left + label_w + row_gap;
                        let mut rects = Vec::new();
                        for w in widths {
                            rects.push(Rect::new(x, top, w, row_h));
                            x += w + row_gap;
                        }
                        RowLayout {
                            rect,
                            label,
                            controls: rects,
                            lines: Vec::new(),
                        }
                    }
                };
                rows.push(placed);
            }
            let drawn = used.iter().copied().fold(0.0, f32::max);
            let content = if model.rows_per_column > 0 {
                model.rows_per_column as f32 * pitch
            } else {
                drawn
            }
            .max(drawn);
            y += content + gap;
            // What this page has less than the tallest one (rows, the notice line): the panel stands
            // where the tallest would, so the tabs stay in place from page to page.
            slack = (model.rows_reserved as f32 * pitch - content).max(0.0);

            if model.notice.is_some() {
                let h = px(p.notice_height_px, scale);
                notice = Some(Rect::new(x0, y, inner_w, h));
                y += h + gap;
            } else if model.rows_reserved > 0 {
                slack += px(p.notice_height_px, scale) + gap;
            }

            let h = px(look.footer.height_px, scale);
            let n = model.footer.len();
            if n > 0 {
                let want = px(look.footer.button_width_px, scale);
                let each = want.min(((inner_w - gap * (n as f32 - 1.0)) / n as f32).floor());
                // The last button (Back) stands at the right end, the others from the left.
                for i in 0..n - 1 {
                    footer.push(Rect::new(x0 + i as f32 * (each + gap), y, each, h));
                }
                footer.push(Rect::new(x0 + inner_w - each, y, each, h));
            }
            y += h + gap;
        }
    }
    let panel_h = (y - gap + pad).max(title_h + 2.0 * pad);
    let panel = Rect::new(
        screen.x + ((screen.w - panel_w) * 0.5).floor(),
        // Centred as the tallest page of the settings would be, so a shorter page stands at its top.
        screen.y + ((screen.h - (panel_h + slack)) * 0.5).floor().max(0.0),
        panel_w,
        panel_h,
    );
    let at = |r: Rect| Rect::new(panel.x + r.x, panel.y + r.y, r.w, r.h);
    Layout {
        panel,
        title: at(title),
        buttons: buttons.into_iter().map(at).collect(),
        tabs: tabs.into_iter().map(at).collect(),
        rows: rows
            .into_iter()
            .map(|r| RowLayout {
                rect: at(r.rect),
                label: at(r.label),
                controls: r.controls.into_iter().map(at).collect(),
                lines: r.lines,
            })
            .collect(),
        notice: notice.map(at),
        footer: footer.into_iter().map(at).collect(),
    }
}

/// The item under `(x, y)` in a laid-out menu, `None` on the panel's blank parts and outside it. A
/// disabled control answers too (it is [`Item::Noop`] or does nothing when clicked).
pub fn hit(model: &Model, layout: &Layout, x: f32, y: f32) -> Option<Item> {
    let mut found = None;
    let mut test = |rect: &Rect, item: Item| {
        if rect.contains(x, y) {
            found = Some(item);
        }
    };
    for (rect, c) in layout.buttons.iter().zip(&model.buttons) {
        test(rect, c.item);
    }
    for (rect, c) in layout.tabs.iter().zip(&model.tabs) {
        test(rect, c.item);
    }
    for (placed, row) in layout.rows.iter().zip(&model.rows) {
        if let Row::Setting { controls, .. } = row {
            for (rect, c) in placed.controls.iter().zip(controls) {
                test(rect, c.item);
            }
        }
    }
    for (rect, c) in layout.footer.iter().zip(&model.footer) {
        test(rect, c.item);
    }
    found
}

/// Draws one control: a button with the ring of a current choice and, for a colour choice, its
/// swatches before the text.
fn control(
    ui: &mut Ui,
    look: &MenuLook,
    id: &str,
    rect: Rect,
    c: &crate::menu::Control,
    clicked: &mut Option<Item>,
) {
    let scale = ui.scale();
    if c.selected {
        let ring = px(look.selected.ring_px, scale);
        let outer = Rect::new(
            rect.x - ring,
            rect.y - ring,
            rect.w + 2.0 * ring,
            rect.h + 2.0 * ring,
        );
        ui.fill(outer, color(look.selected.ring));
    }
    let plain = c.swatches.is_empty();
    let label = if plain { c.text.as_str() } else { "" };
    let response = ui.button(id, rect, label, c.enabled);
    if response.clicked && c.enabled {
        *clicked = Some(c.item);
    }
    if !plain {
        let size = px(look.swatch.size_px, scale);
        let gap = (size * 0.3).round().max(1.0);
        let outline = (scale.round()).max(1.0);
        let mut x = rect.x + (8.0 * scale).round();
        let y = rect.y + ((rect.h - size) * 0.5).floor();
        for rgb in &c.swatches {
            ui.fill(
                Rect::new(
                    x - outline,
                    y - outline,
                    size + 2.0 * outline,
                    size + 2.0 * outline,
                ),
                color(look.swatch.outline),
            );
            ui.fill(
                Rect::new(x, y, size, size),
                Color::rgb(rgb[0], rgb[1], rgb[2]),
            );
            x += size + gap;
        }
        let text_px = ui.text_px();
        let (_, th) = ui.measure(&c.text);
        let text_color = if c.enabled {
            ui.config().text
        } else {
            ui.config().text_disabled
        };
        ui.text_px_at(
            x + gap,
            (rect.y + (rect.h - th) * 0.5).round(),
            &c.text,
            text_px,
            text_color,
        );
    }
}

/// Draws `model` on `ui` and returns the button that was clicked this frame, if one was. The caller
/// has already given the UI the real pointer input (the game's panels were built blind).
pub fn frame(ui: &mut Ui, model: &Model, look: &MenuLook) -> Option<Item> {
    let screen = ui.screen();
    let scale = ui.scale();
    let measure = |t: &str| ui.measure(t).0;
    let lay = layout(model, look, screen, scale, &measure);
    let mut clicked = None;

    // The window behind: dimmed, and nothing in it is the game's.
    ui.fill(screen, color(look.dim));
    ui.block_rect(screen);
    // The panel with a frame round it.
    let border = px(look.panel.border_px, scale).max(1.0);
    let p = lay.panel;
    ui.fill(
        Rect::new(
            p.x - border,
            p.y - border,
            p.w + 2.0 * border,
            p.h + 2.0 * border,
        ),
        color(look.panel.border),
    );
    ui.panel_rect(p);

    let title_px = f32::from(look.panel.title_px) * scale;
    let (tw, th) = ui.measure_px(&model.title, title_px);
    ui.text_px_at(
        lay.title.x + (lay.title.w - tw) * 0.5,
        lay.title.y + (lay.title.h - th) * 0.5,
        &model.title,
        title_px,
        color(look.panel.title),
    );

    let mut n = 0;
    let mut next_id = || {
        n += 1;
        format!("menu-{n}")
    };
    for (rect, c) in lay.buttons.iter().zip(&model.buttons) {
        control(ui, look, &next_id(), *rect, c, &mut clicked);
    }
    for (rect, c) in lay.tabs.iter().zip(&model.tabs) {
        control(ui, look, &next_id(), *rect, c, &mut clicked);
    }
    let text_px = ui.text_px();
    for (placed, row) in lay.rows.iter().zip(&model.rows) {
        match row {
            Row::Spacer => {}
            Row::Heading(text) => {
                let (_, th) = ui.measure(text);
                ui.text_px_at(
                    placed.label.x,
                    placed.label.y + ((placed.label.h - th) * 0.5).round(),
                    text,
                    text_px,
                    color(look.row.heading),
                );
            }
            Row::Note(_) => {
                let line_h = (px(look.row.height_px, scale) * 0.8).round();
                for (i, line) in placed.lines.iter().enumerate() {
                    ui.text_px_at(
                        placed.rect.x,
                        placed.rect.y + i as f32 * line_h,
                        line,
                        text_px,
                        color(look.panel.hint),
                    );
                }
            }
            Row::Setting { label, controls } => {
                let (_, th) = ui.measure(label);
                ui.text_px_at(
                    placed.label.x,
                    placed.label.y + ((placed.label.h - th) * 0.5).round(),
                    label,
                    text_px,
                    ui.config().text,
                );
                for (rect, c) in placed.controls.iter().zip(controls) {
                    control(ui, look, &next_id(), *rect, c, &mut clicked);
                }
            }
        }
    }
    if let (Some(rect), Some(notice)) = (lay.notice, &model.notice) {
        let (_, th) = ui.measure(&notice.text);
        let tone = if notice.warn {
            look.panel.notice
        } else {
            look.panel.hint
        };
        ui.text_px_at(
            rect.x,
            rect.y + ((rect.h - th) * 0.5).round(),
            &notice.text,
            text_px,
            color(tone),
        );
    }
    for (rect, c) in lay.footer.iter().zip(&model.footer) {
        control(ui, look, &next_id(), *rect, c, &mut clicked);
    }
    clicked
}

/// Starts the frame of a UI that may have a menu over the game (WP11.5a). Returns the input the game's
/// widgets are built with: `real`, or, while a menu is open, an input that sees nothing (no cursor, no
/// button), so nothing under the menu can be hovered, pressed or clicked.
pub fn begin_frame(
    ui: &mut Ui,
    real: UiInput,
    screen: (u32, u32),
    dpi: f64,
    menu_open: bool,
) -> UiInput {
    let game = if menu_open { UiInput::default() } else { real };
    ui.begin_frame(game, screen, dpi);
    game
}

/// Ends the frame begun by [`begin_frame`]: draws `menu` (if one is open) over the game's panels
/// with the real input, and decides what the UI consumed. Returns the button of the menu that was
/// clicked, if one was.
pub fn finish_frame(ui: &mut Ui, real: UiInput, menu: Option<&Menu>) -> Option<Item> {
    let mut clicked = None;
    if let Some(menu) = menu {
        ui.set_input(real);
        let look = menu.look();
        let fit = fit(ui.screen(), ui.scale(), look);
        if let Some(model) = menu.model(fit) {
            clicked = frame(ui, &model, look);
        }
    }
    ui.end_frame();
    clicked
}

/// The two buttons under the banner of a decided match (Restart, Main menu): drawn when no menu is
/// open, and the button that was clicked is returned. `banner_bottom_px` is where the banner ends in
/// logical pixels (its top and height, `match_look.ron`).
pub fn result_buttons(ui: &mut Ui, look: &MenuLook, banner_bottom_px: u16) -> Option<Item> {
    let r = &look.result_buttons;
    let top = banner_bottom_px.saturating_add(r.below_banner_px);
    let size = (f32::from(r.width_px), f32::from(r.height_px));
    let gap = f32::from(r.gap_px);
    let scale = ui.scale();
    let screen = ui.screen();
    // The pair is centred: the first button left of the middle, the second right of it.
    let left = ui.place(
        screen,
        Anchor::Top,
        (-(size.0 + gap) * 0.5, f32::from(top)),
        size,
    );
    let right = ui.place(
        screen,
        Anchor::Top,
        ((size.0 + gap) * 0.5, f32::from(top)),
        size,
    );
    let _ = scale;
    let mut clicked = None;
    let w = &look.words;
    for (id, rect, text, item) in [
        ("result-restart", left, &w.restart, Item::Restart),
        ("result-main-menu", right, &w.main_menu, Item::MainMenu),
    ] {
        if ui.button(id, rect, text, true).clicked {
            clicked = Some(item);
        }
    }
    clicked
}

#[cfg(test)]
mod tests;
