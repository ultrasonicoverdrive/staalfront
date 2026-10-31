//! The ore a harvester carries, as a fill bar under it (WP7.2). Presentation only: the load is
//! the simulation's (`World::harvester_of`), copied into every snapshot
//! ([`SimSnapshot::cargo_of`]); nothing here reads or changes anything but the picture.
//!
//! Every harvester has the bar, selected or not (a harvester is the one unit whose state the
//! player watches without selecting it): a dark track with an amber fill that grows from the left
//! as the harvester mines and shrinks, in bites, as the refinery unloads it. Under the model it
//! sits just below the ground point of the box; beyond the models, under the icon. All of it goes
//! on the world layer of `sf_ui`, in the same draw call as the rings and health bars. A world
//! without harvesters skips the pass altogether.

use sf_render::glam::Vec3;
use sf_render::icons::Dominant;
use sf_render::pick::project_px;
use sf_render::{DrawnUnit, Instancer};
use sf_ui::{Color, Layer, Rect};

use crate::selection_view::{FrameGeometry, entity_of};
use crate::sim_snapshot::SimSnapshot;

/// Height of the bar, in logical pixels.
const BAR_H_PX: f32 = 4.0;
/// Narrowest and widest bar, in logical pixels.
const BAR_MIN_W_PX: f32 = 22.0;
const BAR_MAX_W_PX: f32 = 90.0;
/// Gap between the unit and its bar, in logical pixels.
const BAR_GAP_PX: f32 = 3.0;

/// The fill: the colour of ore.
pub const FILL: Color = Color(240, 170, 40, 255);
/// The fill of a full load.
pub const FULL: Color = Color(255, 205, 70, 255);

/// How many pixels of a bar `width` pixels wide a load of `load` out of `capacity` fills. A load
/// above the capacity fills the bar; a capacity of zero fills nothing.
pub fn filled_px(load: u16, capacity: u16, width: f32) -> f32 {
    if capacity == 0 {
        return 0.0;
    }
    (width * f32::from(load.min(capacity)) / f32::from(capacity)).round()
}

/// The fill colour: brighter when the harvester is full.
pub fn fill_color(load: u16, capacity: u16) -> Color {
    if capacity > 0 && load >= capacity {
        FULL
    } else {
        FILL
    }
}

/// Draws the cargo bar of every harvester of the last frame on `layer`. Does nothing, not even a
/// pass over the units, when the snapshot has no harvester.
pub fn draw(
    instancer: &Instancer,
    snap: &SimSnapshot,
    frame: &FrameGeometry,
    layer: &mut Layer<'_>,
) {
    if !snap.has_harvesters() {
        return;
    }
    instancer.visit_drawn(
        |id| snap.cargo_of(entity_of(id)).is_some(),
        |unit| draw_unit(layer, frame, snap, unit),
    );
}

fn draw_unit(layer: &mut Layer<'_>, frame: &FrameGeometry, snap: &SimSnapshot, unit: DrawnUnit) {
    let Some((load, capacity)) = snap.cargo_of(entity_of(unit.id)) else {
        return;
    };
    let scale = frame.ui_scale;
    let bar_h = (BAR_H_PX * scale).round().max(2.0);
    let gap = BAR_GAP_PX * scale;
    let (cx, top, bar_w) = if frame.blend.dominant() == Dominant::Model {
        let centre = Vec3::from(unit.centre);
        let Some((p, _)) = project_px(&frame.view_proj, centre, frame.viewport) else {
            return;
        };
        // The width follows the model: its box across, as the picture sees it.
        let half = centre + Vec3::new(0.5 * unit.size[0].max(unit.size[2]), 0.0, 0.0);
        let width = project_px(&frame.view_proj, half, frame.viewport)
            .map_or(0.0, |(q, _)| 2.0 * (q[0] - p[0]).abs());
        (p[0], p[1] + gap, 1.2 * width)
    } else {
        let Some((p, _)) = project_px(
            &frame.view_proj,
            Vec3::from(unit.icon_anchor),
            frame.viewport,
        ) else {
            return;
        };
        (p[0], p[1] + 0.5 * frame.icon_px + gap, 1.2 * frame.icon_px)
    };
    let bar_w = bar_w
        .clamp(BAR_MIN_W_PX * scale, BAR_MAX_W_PX * scale)
        .round();
    let (x, y) = ((cx - 0.5 * bar_w).round(), top.round());
    let one = scale.round().max(1.0);
    layer.fill(
        Rect::new(x - one, y - one, bar_w + 2.0 * one, bar_h + 2.0 * one),
        Color(0, 0, 0, 190),
    );
    layer.fill(Rect::new(x, y, bar_w, bar_h), Color(40, 40, 44, 255));
    let filled = filled_px(load, capacity, bar_w);
    if filled > 0.0 {
        layer.fill(Rect::new(x, y, filled, bar_h), fill_color(load, capacity));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fill_is_the_share_of_the_capacity() {
        assert_eq!(filled_px(0, 200, 40.0), 0.0);
        assert_eq!(filled_px(100, 200, 40.0), 20.0);
        assert_eq!(filled_px(200, 200, 40.0), 40.0);
        assert_eq!(filled_px(50, 200, 40.0), 10.0);
    }

    #[test]
    fn a_load_above_the_capacity_fills_the_bar_and_no_capacity_fills_nothing() {
        assert_eq!(filled_px(500, 200, 40.0), 40.0);
        assert_eq!(filled_px(10, 0, 40.0), 0.0);
    }

    #[test]
    fn a_full_load_has_its_own_colour() {
        assert_eq!(fill_color(199, 200), FILL);
        assert_eq!(fill_color(200, 200), FULL);
        assert_ne!(FILL, FULL);
    }
}
