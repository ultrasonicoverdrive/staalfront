//! The ore storage as a fill bar under every structure that stores ore (WP7.3). Presentation only:
//! the stored ore and the capacity are the simulation's (`World::ore_stored`,
//! `World::storage_capacity`), copied into every snapshot ([`SimSnapshot::storage_of`]); nothing
//! here reads or changes anything but the picture.
//!
//! A player's ore is one store, kept in all its standing storing structures together, so every
//! refinery of a player shows the same fill: stored out of capacity. The bar lies where the
//! construction bar lies (under the front edge of the footprint, [`construction_view::bar_rects`]),
//! which a complete structure no longer has. A full store is drawn in its own colour: the
//! harvesters wait. The look is `assets/config/storage_look.ron`.

use sf_data::storage_look::StorageLook;
use sf_ui::{Color, Layer};

use crate::construction_view::{self, front_edge};
use crate::placement_view::GhostFrame;
use crate::sim_snapshot::SimSnapshot;

/// The shipped look, read once.
pub fn shipped_look() -> &'static StorageLook {
    static LOOK: std::sync::OnceLock<StorageLook> = std::sync::OnceLock::new();
    LOOK.get_or_init(StorageLook::embedded)
}

/// The fill in thousandths of the bar: `stored` out of `capacity`, at most 1000. A store with no
/// capacity left (its last refinery gone) but ore in it is full; an empty one is empty.
pub fn fill_permille(stored: u64, capacity: u64) -> u16 {
    if capacity == 0 {
        return if stored > 0 { 1000 } else { 0 };
    }
    let p = stored.min(capacity).saturating_mul(1000) / capacity;
    u16::try_from(p).unwrap_or(1000)
}

/// The fill colour: the look's own colour for a full store.
pub fn fill_color(stored: u64, capacity: u64, look: &StorageLook) -> Color {
    let c = if stored >= capacity && stored > 0 {
        look.full
    } else {
        look.bar.fill
    };
    Color(c.0, c.1, c.2, c.3)
}

/// Draws the bar of every storing structure of `snap` on `layer`. `height(x, z)` is the terrain
/// height in metres. Returns the number of bars drawn.
pub fn draw_bars(
    layer: &mut Layer<'_>,
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    snap: &SimSnapshot,
    look: &StorageLook,
) -> usize {
    let mut drawn = 0;
    for &id in snap.stores() {
        let Some(owner) = snap.get(id).map(|v| v.owner) else {
            continue;
        };
        let (stored, capacity) = snap.storage_of(owner).unwrap_or((0, 0));
        let Some((left, right)) = front_edge(frame, height, snap, id) else {
            continue;
        };
        let permille = fill_permille(stored, capacity);
        let Some(rects) =
            construction_view::bar_rects(left, right, permille, &look.bar, frame.ui_scale)
        else {
            continue;
        };
        let b = look.bar.back;
        layer.fill_quad(rects.back, Color(b.0, b.1, b.2, b.3));
        if let Some(fill) = rects.fill {
            layer.fill_quad(fill, fill_color(stored, capacity, look));
        }
        drawn += 1;
    }
    drawn
}

/// The line the F1 panel shows for one player's store.
pub fn readout(player: u8, stored: u64, capacity: u64) -> String {
    let state = if capacity == 0 || stored >= capacity {
        " (full)"
    } else {
        ""
    };
    format!("ore store player {player}: {stored} / {capacity}{state}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fill_is_the_share_of_the_capacity() {
        assert_eq!(fill_permille(0, 2000), 0);
        assert_eq!(fill_permille(500, 2000), 250);
        assert_eq!(fill_permille(2000, 2000), 1000);
        // More than fits (a refinery was lost): a full bar.
        assert_eq!(fill_permille(1200, 600), 1000);
        // No capacity left: full with ore in it, empty without.
        assert_eq!(fill_permille(5, 0), 1000);
        assert_eq!(fill_permille(0, 0), 0);
    }

    #[test]
    fn a_full_store_has_its_own_colour() {
        let look = StorageLook::embedded();
        let f = |c: (u8, u8, u8, u8)| Color(c.0, c.1, c.2, c.3);
        assert_eq!(fill_color(1999, 2000, &look), f(look.bar.fill));
        assert_eq!(fill_color(2000, 2000, &look), f(look.full));
        assert_eq!(fill_color(1200, 600, &look), f(look.full));
        assert_eq!(fill_color(0, 0, &look), f(look.bar.fill));
    }

    #[test]
    fn the_readout_says_stored_out_of_capacity() {
        assert_eq!(readout(0, 600, 2000), "ore store player 0: 600 / 2000");
        assert_eq!(
            readout(1, 2000, 2000),
            "ore store player 1: 2000 / 2000 (full)"
        );
        assert_eq!(
            readout(0, 1200, 600),
            "ore store player 0: 1200 / 600 (full)"
        );
    }
}
