//! Drawing the sidebar and reading its clicks (WP11.1, `docs/rules/sidebar.md`). The model
//! ([`SidebarModel`]) says what to show, the layout ([`crate::sidebar_layout`]) where; this builds
//! the quads on the UI and turns a press on a tab or an item into a state change or an
//! [`Action`]. Every size, word and colour is `assets/config/sidebar.ron`
//! ([`sf_data::sidebar_look`]).
//!
//! The whole column is one blocking panel: a click anywhere on it is the UI's, so it never
//! reaches the map (no selection box, no order behind it). A press acts when the button goes
//! down, as the genre does.

use sf_data::money_look::MoneyLook;
use sf_data::sidebar_look::{Rgba, SidebarLook};
use sf_ui::{Color, Rect, Ui, UiInput};

use crate::money_view;
use crate::sidebar::{Action, Button, Item, SidebarModel, Tab, power_fill};
use crate::sidebar_layout::{self, Hit, Layout};

/// The shipped look, read once.
pub fn shipped_look() -> &'static SidebarLook {
    static LOOK: std::sync::OnceLock<SidebarLook> = std::sync::OnceLock::new();
    LOOK.get_or_init(SidebarLook::embedded)
}

/// What the sidebar remembers between frames: the tab that is shown and the first row of items.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SidebarState {
    /// The tab that is shown.
    pub tab: Tab,
    /// The first row of items that is shown.
    pub scroll: usize,
}

impl Default for SidebarState {
    fn default() -> Self {
        SidebarState {
            tab: Tab::Structures,
            scroll: 0,
        }
    }
}

fn color(c: Rgba) -> Color {
    Color(c.0, c.1, c.2, c.3)
}

/// The colour of a picture tile.
fn tile_color(c: (u8, u8, u8)) -> Color {
    Color(c.0, c.1, c.2, 255)
}

/// Text with its box centred in `rect`.
fn centered(ui: &mut Ui, rect: Rect, text: &str, px: f32, c: Color) {
    let (w, h) = ui.measure_px(text, px);
    ui.text_px_at(
        (rect.x + (rect.w - w) * 0.5).round(),
        (rect.y + (rect.h - h) * 0.5).round(),
        text,
        px,
        c,
    );
}

/// A frame of `thickness` px round `rect`.
fn ring(ui: &mut Ui, rect: Rect, thickness: f32, c: Color) {
    let t = thickness.max(1.0);
    ui.fill(Rect::new(rect.x, rect.y, rect.w, t), c);
    ui.fill(Rect::new(rect.x, rect.bottom() - t, rect.w, t), c);
    ui.fill(Rect::new(rect.x, rect.y + t, t, rect.h - 2.0 * t), c);
    ui.fill(
        Rect::new(rect.right() - t, rect.y + t, t, rect.h - 2.0 * t),
        c,
    );
}

/// What the sidebar is drawn with besides the model.
#[derive(Copy, Clone, Debug)]
pub struct Setup<'a> {
    /// `sidebar.ron`.
    pub look: &'a SidebarLook,
    /// `money_look.ron`: the word and the colours of the credits.
    pub money: &'a MoneyLook,
    /// The width of the edge-scroll band (`input.ron`), in physical pixels.
    pub edge_band_px: f32,
    /// The minimap sits under the column.
    pub minimap: bool,
}

/// Builds the sidebar on `ui` for this frame and returns what the player asked for. `input` is the
/// frame's pointer input (the UI has its own copy; the sidebar needs the wheel and the presses).
pub fn frame(
    ui: &mut Ui,
    input: &UiInput,
    state: &mut SidebarState,
    model: &SidebarModel,
    setup: &Setup<'_>,
) -> Vec<Action> {
    let Setup {
        look,
        money,
        edge_band_px,
        minimap,
    } = *setup;
    let scale = ui.scale();
    let lay = sidebar_layout::layout(ui.screen(), scale, look, edge_band_px, minimap);
    let cursor = input.cursor;
    let over = |r: Rect| cursor.is_some_and(|(x, y)| r.contains(x, y));

    // The wheel over the column scrolls the items.
    if input.wheel != 0.0 && over(lay.column) {
        state.scroll = sidebar_layout::scrolled(
            state.scroll,
            input.wheel,
            model.items.len(),
            lay.columns,
            lay.rows_visible,
        );
    }
    state.scroll = sidebar_layout::clamp_scroll(
        state.scroll,
        model.items.len(),
        lay.columns,
        lay.rows_visible,
    );

    ui.panel_rect(lay.column);
    draw_money(ui, &lay, model, look, money);
    draw_power(ui, &lay, model, look);
    draw_tabs(ui, &lay, model, state.tab, look, cursor);
    draw_items(ui, &lay, model, state.scroll, look, cursor);
    if let Some(hint) = &model.empty_hint {
        draw_empty_hint(ui, &lay, hint, look);
    }

    // The clicks. A press on a tab shows it; a press on an item is its action.
    let mut actions = Vec::new();
    if let Some((x, y)) = cursor {
        for (button, index) in [(Button::Left, 0), (Button::Right, 2)] {
            if !input.pressed[index] {
                continue;
            }
            match lay.hit(x, y, state.scroll, model.items.len()) {
                Some(Hit::Tab(tab)) if button == Button::Left => {
                    state.tab = tab;
                    state.scroll = 0;
                }
                Some(Hit::Item(i)) => actions.extend(model.items[i].click(button)),
                _ => {}
            }
        }
    }
    actions
}

fn draw_money(
    ui: &mut Ui,
    lay: &Layout,
    model: &SidebarModel,
    look: &SidebarLook,
    money: &MoneyLook,
) {
    ui.fill(lay.money, color(look.colors.power_back));
    let Some(credits) = model.credits else { return };
    let px = f32::from(look.text_px) * ui.scale() * 1.15;
    let text = money_view::readout_text(money, credits);
    let c = money_view::readout_color(money, credits, model.waiting_for_credits);
    let (_, h) = ui.measure_px(&text, px);
    ui.text_px_at(
        lay.money.x + (6.0 * ui.scale()).round(),
        (lay.money.y + (lay.money.h - h) * 0.5).round(),
        &text,
        px,
        c,
    );
}

fn draw_power(ui: &mut Ui, lay: &Layout, model: &SidebarModel, look: &SidebarLook) {
    let c = &look.colors;
    let scale = ui.scale();
    let px = f32::from(look.small_px) * scale;
    ui.fill(lay.power, color(c.power_back));
    match model.power {
        None => {
            // No power rules (WP6.4 is not built): a neutral frame that says so.
            ring(ui, lay.power, scale.round(), color(c.power_neutral));
            let text = format!("{}: {}", look.words.power, look.words.power_none);
            centered(ui, lay.power, &text, px, color(c.text_dim));
        }
        Some(reading) => {
            let (permille, low) = power_fill(reading);
            let fill = Rect::new(
                lay.power.x,
                lay.power.y,
                (lay.power.w * f32::from(permille) / 1000.0).round(),
                lay.power.h,
            );
            ui.fill(fill, color(if low { c.power_low } else { c.power_fill }));
            let text = format!("{}: {}/{}", look.words.power, reading.used, reading.made);
            centered(ui, lay.power, &text, px, color(c.text));
        }
    }
}

fn draw_tabs(
    ui: &mut Ui,
    lay: &Layout,
    model: &SidebarModel,
    shown: Tab,
    look: &SidebarLook,
    cursor: Option<(f32, f32)>,
) {
    let c = &look.colors;
    let px = f32::from(look.text_px) * ui.scale();
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let rect = lay.tabs[i];
        let hovered = cursor.is_some_and(|(x, y)| rect.contains(x, y));
        let back = if tab == shown {
            c.tab_active
        } else if hovered {
            c.tab_hover
        } else {
            c.tab
        };
        ui.fill(rect, color(back));
        let text_color = if model.counts[i] == 0 && tab != shown {
            c.tab_empty_text
        } else {
            c.text
        };
        centered(ui, rect, tab.pick(&look.tab_labels), px, color(text_color));
    }
}

fn draw_items(
    ui: &mut Ui,
    lay: &Layout,
    model: &SidebarModel,
    scroll: usize,
    look: &SidebarLook,
    cursor: Option<(f32, f32)>,
) {
    for n in 0..lay.visible_slots() {
        let index = scroll * lay.columns + n;
        let Some(item) = model.items.get(index) else {
            break;
        };
        let rect = lay.slot(n);
        let words = model.waiting_words[index].as_deref();
        draw_tile(
            ui,
            rect,
            item,
            words,
            model.chosen == Some(item.kind),
            look,
            cursor,
        );
    }
}

/// Draws the hint of an empty tab in the item area.
fn draw_empty_hint(ui: &mut Ui, lay: &Layout, text: &str, look: &SidebarLook) {
    let px = f32::from(look.small_px) * ui.scale();
    let rect = Rect::new(
        lay.area.x,
        lay.area.y,
        lay.area.w,
        lay.tile.1.min(lay.area.h),
    );
    centered(ui, rect, text, px, color(look.colors.text_dim));
}

fn draw_tile(
    ui: &mut Ui,
    rect: Rect,
    item: &Item,
    words: Option<&str>,
    chosen: bool,
    look: &SidebarLook,
    cursor: Option<(f32, f32)>,
) {
    let c = &look.colors;
    let scale = ui.scale();
    let small = f32::from(look.small_px) * scale;
    let bar_h = (f32::from(look.bar_height_px) * scale).round().max(1.0);
    let strip_h = (small * 1.5).round();

    ui.fill(rect, color(c.tile_back));
    // The picture: a coloured tile with the short name.
    let picture = Rect::new(
        rect.x + 1.0,
        rect.y + 1.0,
        rect.w - 2.0,
        (rect.h - strip_h - bar_h - 1.0).max(0.0),
    );
    ui.fill(picture, tile_color(item.picture.color));
    centered(
        ui,
        picture,
        &item.picture.label,
        f32::from(look.label_px) * scale,
        color(c.text),
    );
    // The count of this item that is going up or waits in a queue, top right.
    if item.count > 0 {
        let text = item.count.to_string();
        let (w, h) = ui.measure_px(&text, small);
        let pad = (3.0 * scale).round();
        let badge = Rect::new(
            picture.right() - w - 2.0 * pad,
            picture.y,
            w + 2.0 * pad,
            h + pad,
        );
        ui.fill(badge, color(c.badge_back));
        ui.text_px_at(
            badge.x + pad,
            badge.y + (pad * 0.5).round(),
            &text,
            small,
            color(c.text),
        );
    }
    // The strip under the picture: the price left, why it waits right.
    let strip_y = picture.bottom() + 1.0;
    let (_, th) = ui.measure_px("0", small);
    let ty = (strip_y + (strip_h - th) * 0.5).round();
    let price_color = if item.affordable { c.text_dim } else { c.warn };
    ui.text_px_at(
        rect.x + (3.0 * scale).round(),
        ty,
        &item.cost.to_string(),
        small,
        color(price_color),
    );
    // On the right: why it waits, else how far along it is in percent.
    let right_text = match (words, item.progress) {
        (Some(words), _) => Some((words.to_owned(), c.warn)),
        (None, Some(permille)) => Some((format!("{}%", permille / 10), c.text)),
        (None, None) => None,
    };
    if let Some((text, text_color)) = right_text {
        let (w, _) = ui.measure_px(&text, small);
        ui.text_px_at(
            (rect.right() - w - (3.0 * scale).round()).round(),
            ty,
            &text,
            small,
            color(text_color),
        );
    }
    // The progress bar along the bottom edge.
    if let Some(permille) = item.progress {
        let bar = Rect::new(rect.x, rect.bottom() - bar_h, rect.w, bar_h);
        ui.fill(bar, color(c.bar_back));
        let fill = (bar.w * f32::from(permille.min(1000)) / 1000.0).round();
        let fill_color = if item.waiting.is_some() {
            c.bar_waiting
        } else {
            c.bar_fill
        };
        ui.fill(Rect::new(bar.x, bar.y, fill, bar.h), color(fill_color));
    }
    if chosen {
        ring(ui, rect, (2.0 * scale).round(), color(c.tile_chosen));
    }
    if cursor.is_some_and(|(x, y)| rect.contains(x, y)) {
        ui.fill(rect, color(c.tile_hover));
    }
}

#[cfg(test)]
mod tests;
