//! Where the parts of the sidebar are (WP11.1): the geometry of the column, the hit test from a
//! click to a tab or an item, and the scrolling of the item grid. Pure functions of the window
//! size, the UI scale and `sidebar.ron`; no window and no drawing, so a test can lay the sidebar
//! out at any size and click on it.
//!
//! The column lies on the right edge of the window with a margin of at least the edge-scroll band
//! (`input.ron`, `edge_margin`), so the pixels that scroll the camera are never the sidebar's: a
//! cursor is either on the sidebar or in the band, never both (progress item 22).

use sf_data::sidebar_look::SidebarLook;
use sf_ui::Rect;

use crate::sidebar::Tab;

/// The rectangles of the sidebar, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The whole column; it takes every click inside it.
    pub column: Rect,
    /// The credits.
    pub money: Rect,
    /// The power bar.
    pub power: Rect,
    /// The four tabs, in [`Tab::ALL`] order.
    pub tabs: [Rect; 4],
    /// The grid of items.
    pub area: Rect,
    /// Size of one item.
    pub tile: (f32, f32),
    /// Space between items.
    pub gap: f32,
    /// Items per row.
    pub columns: usize,
    /// Rows of items that fit in the area.
    pub rows_visible: usize,
}

/// What a point on the sidebar is on.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A tab.
    Tab(Tab),
    /// The item with this index in the tab's list (scrolling already taken into account).
    Item(usize),
    /// The column, but nothing that does anything.
    Chrome,
}

fn px(v: u16, scale: f32) -> f32 {
    (f32::from(v) * scale).round()
}

/// Lays the sidebar out in `screen` (physical pixels) at UI `scale`. `edge_band_px` is the width of
/// the edge-scroll band; the margin is never less. With `minimap` the column stops above the
/// minimap's room, else it runs down to the margin.
pub fn layout(
    screen: Rect,
    scale: f32,
    look: &SidebarLook,
    edge_band_px: f32,
    minimap: bool,
) -> Layout {
    let margin = px(look.margin_px, scale).max(edge_band_px.ceil());
    let (gap, pad) = (px(look.gap_px, scale), px(look.pad_px, scale));
    let width = px(look.width_px, scale);
    let top = margin;
    let bottom = screen.bottom()
        - if minimap {
            px(look.bottom_reserve_px, scale)
        } else {
            margin
        };
    let column = Rect::new(
        screen.right() - margin - width,
        top,
        width,
        (bottom - top).max(0.0),
    );
    let inner_x = column.x + pad;
    let inner_w = (column.w - 2.0 * pad).max(0.0);

    let mut y = column.y + pad;
    let money = Rect::new(inner_x, y, inner_w, px(look.money_height_px, scale));
    y += money.h + gap;
    let power = Rect::new(inner_x, y, inner_w, px(look.power_height_px, scale));
    y += power.h + gap;

    let tab_columns = usize::from(look.tab_columns);
    let tab_w = ((inner_w - gap * (tab_columns as f32 - 1.0)) / tab_columns as f32).floor();
    let tab_h = px(look.tab_height_px, scale);
    let mut tabs = [Rect::default(); 4];
    for (i, tab) in tabs.iter_mut().enumerate() {
        let (col, row) = (i % tab_columns, i / tab_columns);
        *tab = Rect::new(
            inner_x + col as f32 * (tab_w + gap),
            y + row as f32 * (tab_h + gap),
            tab_w,
            tab_h,
        );
    }
    let tab_rows = Tab::ALL.len().div_ceil(tab_columns);
    y += tab_rows as f32 * (tab_h + gap);

    let columns = usize::from(look.tile_columns);
    let tile_w = ((inner_w - gap * (columns as f32 - 1.0)) / columns as f32).floor();
    let tile_h = px(look.tile_height_px, scale);
    let area_h = (column.bottom() - pad - y).max(0.0);
    let rows_visible = if area_h < tile_h {
        0
    } else {
        ((area_h + gap) / (tile_h + gap)).floor() as usize
    };
    Layout {
        column,
        money,
        power,
        tabs,
        area: Rect::new(inner_x, y, inner_w, area_h),
        tile: (tile_w, tile_h),
        gap,
        columns,
        rows_visible,
    }
}

impl Layout {
    /// The rectangle of the `n`-th visible slot, row by row from the top left of the grid.
    pub fn slot(&self, n: usize) -> Rect {
        let (col, row) = (n % self.columns, n / self.columns);
        Rect::new(
            self.area.x + col as f32 * (self.tile.0 + self.gap),
            self.area.y + row as f32 * (self.tile.1 + self.gap),
            self.tile.0,
            self.tile.1,
        )
    }

    /// How many items are on the screen at once.
    pub fn visible_slots(&self) -> usize {
        self.columns * self.rows_visible
    }

    /// What lies at `(x, y)`: `None` outside the column. `scroll` is the first row shown and
    /// `items` how many items the tab has.
    pub fn hit(&self, x: f32, y: f32, scroll: usize, items: usize) -> Option<Hit> {
        if !self.column.contains(x, y) {
            return None;
        }
        if let Some(i) = self.tabs.iter().position(|t| t.contains(x, y)) {
            return Some(Hit::Tab(Tab::ALL[i]));
        }
        for n in 0..self.visible_slots() {
            if self.slot(n).contains(x, y) {
                let index = scroll * self.columns + n;
                return Some(if index < items {
                    Hit::Item(index)
                } else {
                    Hit::Chrome
                });
            }
        }
        Some(Hit::Chrome)
    }
}

/// The first row to show so that it stays inside the list: at most `rows - rows_visible`.
pub fn clamp_scroll(scroll: usize, items: usize, columns: usize, rows_visible: usize) -> usize {
    let rows = items.div_ceil(columns.max(1));
    scroll.min(rows.saturating_sub(rows_visible))
}

/// The first row after the wheel moved by `wheel` notches (positive = away from the user = up):
/// one row per movement, whatever its size, kept inside the list.
pub fn scrolled(
    scroll: usize,
    wheel: f32,
    items: usize,
    columns: usize,
    rows_visible: usize,
) -> usize {
    let moved = if wheel > 0.0 {
        scroll.saturating_sub(1)
    } else if wheel < 0.0 {
        scroll + 1
    } else {
        scroll
    };
    clamp_scroll(moved, items, columns, rows_visible)
}

#[cfg(test)]
mod tests;
