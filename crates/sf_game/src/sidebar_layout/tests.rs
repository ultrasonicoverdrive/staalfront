use super::*;

fn look() -> SidebarLook {
    SidebarLook::embedded()
}

fn screen(w: f32, h: f32) -> Rect {
    Rect::new(0.0, 0.0, w, h)
}

#[test]
fn the_column_lies_on_the_right_edge_above_the_minimap() {
    let l = layout(screen(1280.0, 720.0), 1.0, &look(), 4.0, true);
    // 200 wide, 6 from the right and the top; the minimap (200 + 6 margin) and a gap of 4 are left
    // under it.
    assert_eq!(l.column, Rect::new(1074.0, 6.0, 200.0, 504.0));
    assert_eq!(l.column.bottom(), 720.0 - 210.0);
    // Without a minimap it runs down to the margin.
    let l = layout(screen(1280.0, 720.0), 1.0, &look(), 4.0, false);
    assert_eq!(l.column.bottom(), 714.0);
}

#[test]
fn the_parts_stand_top_to_bottom_money_power_tabs_items() {
    let l = layout(screen(1280.0, 720.0), 1.0, &look(), 4.0, true);
    assert!(l.money.bottom() < l.power.y);
    assert!(l.power.bottom() < l.tabs[0].y);
    assert_eq!(l.tabs[0].y, l.tabs[1].y);
    assert_eq!(l.tabs[2].y, l.tabs[3].y);
    assert!(l.tabs[1].bottom() < l.tabs[2].y);
    assert!(l.tabs[3].bottom() < l.area.y);
    // Two columns of tabs in the order of Tab::ALL: structures left, infantry right.
    assert!(l.tabs[0].x < l.tabs[1].x);
    assert!(l.tabs[2].x < l.tabs[3].x);
    // Everything lies inside the column.
    for r in [l.money, l.power, l.area].into_iter().chain(l.tabs) {
        assert!(r.x >= l.column.x && r.right() <= l.column.right(), "{r:?}");
        assert!(
            r.y >= l.column.y && r.bottom() <= l.column.bottom(),
            "{r:?}"
        );
    }
    // Two columns of items, as many rows as fit.
    assert_eq!(l.columns, 2);
    assert_eq!(l.tile, (94.0, 62.0));
    assert_eq!(l.rows_visible, 5);
    assert_eq!(l.visible_slots(), 10);
    assert!(l.slot(9).bottom() <= l.area.bottom());
}

#[test]
fn the_sidebar_never_covers_the_edge_scroll_band() {
    // At every UI scale, even the one where 6 logical pixels are 3 physical ones, no pixel of the
    // column lies in the band at the right edge or the top (progress item 22).
    let band = f32::from(sf_engine::Bindings::builtin().edge_margin as u16);
    assert_eq!(band, 4.0, "the band of input.ron");
    for scale in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0] {
        let s = screen(1920.0, 1080.0);
        let l = layout(s, scale, &look(), band, true);
        assert!(
            l.column.right() <= s.right() - band,
            "scale {scale}: right {} in the band",
            l.column.right()
        );
        assert!(l.column.y >= band, "scale {scale}: top {}", l.column.y);
        // A cursor in the band is not on the sidebar, and one on it is not in the band.
        let in_band = s.right() - band;
        assert_eq!(l.hit(in_band, 100.0, 0, 3), None);
        assert!(
            l.hit(l.column.right() - 1.0, l.column.y + 1.0, 0, 3)
                .is_some()
        );
    }
    // And the shipped margin is not a coincidence of the scale: it is at least the band.
    assert!(f32::from(look().margin_px) >= band);
}

#[test]
fn the_column_is_laid_out_in_physical_pixels_at_any_scale() {
    for scale in [0.5, 1.0, 1.5, 2.0] {
        let l = layout(screen(1920.0, 1080.0), scale, &look(), 4.0, true);
        for r in [l.column, l.money, l.power, l.area]
            .into_iter()
            .chain(l.tabs)
        {
            for v in [r.x, r.y, r.w, r.h] {
                assert_eq!(v, v.round(), "scale {scale}: {r:?} is not on whole pixels");
            }
        }
        // The tiles are as wide as two fit.
        assert!(l.tile.0 * 2.0 + l.gap <= l.area.w, "scale {scale}");
    }
    // A bigger scale makes the column wider.
    let a = layout(screen(1920.0, 1080.0), 1.0, &look(), 4.0, true);
    let b = layout(screen(1920.0, 1080.0), 2.0, &look(), 4.0, true);
    assert_eq!(b.column.w, 2.0 * a.column.w);
}

#[test]
fn a_tiny_window_gets_no_item_rows_and_does_not_panic() {
    let l = layout(screen(300.0, 200.0), 1.0, &look(), 4.0, true);
    assert_eq!(l.rows_visible, 0);
    assert_eq!(l.visible_slots(), 0);
    // The column has no height left: it takes no clicks.
    assert_eq!(l.column.h, 0.0);
    assert_eq!(l.hit(l.column.x + 1.0, l.column.y + 1.0, 0, 5), None);
    // A window just tall enough for the tabs but no item row: the column takes clicks, no item.
    let l = layout(screen(600.0, 380.0), 1.0, &look(), 4.0, true);
    assert_eq!(l.rows_visible, 0);
    assert_eq!(
        l.hit(l.column.x + 1.0, l.column.y + 1.0, 0, 5),
        Some(Hit::Chrome)
    );
}

#[test]
fn a_click_hits_the_tab_the_item_or_the_column() {
    let l = layout(screen(1280.0, 720.0), 1.0, &look(), 4.0, true);
    let mid = |r: Rect| r.center();
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let (x, y) = mid(l.tabs[i]);
        assert_eq!(l.hit(x, y, 0, 3), Some(Hit::Tab(tab)), "tab {i}");
    }
    // Seven items, the first row shown: slot n is item n.
    for n in 0..7 {
        let (x, y) = mid(l.slot(n));
        assert_eq!(l.hit(x, y, 0, 7), Some(Hit::Item(n)), "slot {n}");
    }
    // A slot beyond the last item is the column, not an item.
    let (x, y) = mid(l.slot(7));
    assert_eq!(l.hit(x, y, 0, 7), Some(Hit::Chrome));
    // The gap between two tiles, the money block and the power bar are the column.
    let gap_x = l.slot(0).right() + l.gap * 0.5;
    assert_eq!(l.hit(gap_x, mid(l.slot(0)).1, 0, 7), Some(Hit::Chrome));
    let (x, y) = mid(l.money);
    assert_eq!(l.hit(x, y, 0, 7), Some(Hit::Chrome));
    let (x, y) = mid(l.power);
    assert_eq!(l.hit(x, y, 0, 7), Some(Hit::Chrome));
    // Outside the column nothing is hit: that click belongs to the map.
    assert_eq!(l.hit(l.column.x - 1.0, 100.0, 0, 7), None);
    assert_eq!(l.hit(10.0, 10.0, 0, 7), None);
    assert_eq!(l.hit(l.column.x + 10.0, l.column.bottom(), 0, 7), None);
}

#[test]
fn scrolling_moves_the_items_under_the_slots() {
    let l = layout(screen(1280.0, 720.0), 1.0, &look(), 4.0, true);
    let (x, y) = l.slot(0).center();
    // 14 items in 7 rows, 5 shown: after two rows the first slot is item 4.
    assert_eq!(l.hit(x, y, 0, 14), Some(Hit::Item(0)));
    assert_eq!(l.hit(x, y, 2, 14), Some(Hit::Item(4)));
    let (x, y) = l.slot(9).center();
    assert_eq!(l.hit(x, y, 2, 14), Some(Hit::Item(13)));
}

#[test]
fn the_wheel_scrolls_a_row_at_a_time_and_stays_inside_the_list() {
    // 14 items, 2 columns = 7 rows; 5 visible: the first row runs 0 to 2.
    assert_eq!(clamp_scroll(9, 14, 2, 5), 2);
    assert_eq!(clamp_scroll(1, 14, 2, 5), 1);
    // Everything fits: no scrolling.
    assert_eq!(clamp_scroll(3, 8, 2, 5), 0);
    assert_eq!(clamp_scroll(3, 0, 2, 5), 0);
    // Wheel down (negative) scrolls down, up scrolls up, one row whatever the size.
    assert_eq!(scrolled(0, -1.0, 14, 2, 5), 1);
    assert_eq!(scrolled(0, -3.5, 14, 2, 5), 1);
    assert_eq!(scrolled(2, -1.0, 14, 2, 5), 2);
    assert_eq!(scrolled(2, 1.0, 14, 2, 5), 1);
    assert_eq!(scrolled(0, 1.0, 14, 2, 5), 0);
    assert_eq!(scrolled(1, 0.0, 14, 2, 5), 1);
}
