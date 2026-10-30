//! The progress bar of a structure under construction (WP6.3). Presentation only: it paints the
//! thousandths the simulation reports in the snapshot ([`SimSnapshot::constructions`]) and decides
//! nothing. The stage *models* are chosen in [`crate::looks`]; this is the bar under the footprint.
//!
//! The bar lies under the front (south) edge of the footprint, as wide as that edge looks on the
//! screen, and is drawn on the UI's world layer, under the panels. A structure whose edge is too
//! small on the screen (zoomed out) gets none.

use sf_data::build_stages::BarLook;
use sf_data::money_look::PausedLook;
use sf_math::render_bridge::METRES_PER_CELL;
use sf_render::glam::Vec3;
use sf_render::pick::project_px;
use sf_ui::{Color, Layer};

use crate::looks::Looks;
use crate::placement_view::GhostFrame;
use crate::sim_snapshot::SimSnapshot;

/// The two rectangles of a bar, as the corners of quads on the screen (clockwise from top left).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BarRects {
    /// The whole bar.
    pub back: [(f32, f32); 4],
    /// The part that is done; `None` at zero.
    pub fill: Option<[(f32, f32); 4]>,
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> [(f32, f32); 4] {
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

/// Where the bar of a structure goes, given the screen positions of the two ends of the front
/// edge of its footprint, how far along it is (thousandths), the bar's look (height, minimum and
/// maximum width, gap, in logical pixels) and the UI scale. `None` when the edge is too small on
/// the screen or not left to right.
pub fn bar_rects(
    left: [f32; 2],
    right: [f32; 2],
    permille: u16,
    look: &BarLook,
    ui_scale: f32,
) -> Option<BarRects> {
    let width = right[0] - left[0];
    if width < f32::from(look.min_width_px) * ui_scale {
        return None;
    }
    let width = width.min(f32::from(look.max_width_px) * ui_scale);
    let mid = (left[0] + right[0]) * 0.5;
    let (x0, x1) = (mid - width * 0.5, mid + width * 0.5);
    let y0 = (left[1] + right[1]) * 0.5 + f32::from(look.gap_px) * ui_scale;
    let y1 = y0 + f32::from(look.height_px) * ui_scale;
    let done = x0 + width * f32::from(permille.min(1000)) / 1000.0;
    Some(BarRects {
        back: rect(x0, y0, x1, y1),
        fill: (permille > 0).then(|| rect(x0, y0, done, y1)),
    })
}

/// Draws the bar of every structure under construction in `snap` on `layer`. `height(x, z)` is the
/// terrain height in metres. A structure that waits for money (WP7.4, [`SimSnapshot::stalled`]) gets
/// the colours and the pause strokes of `paused` instead. Returns the number of bars drawn.
pub fn draw_bars(
    layer: &mut Layer<'_>,
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    snap: &SimSnapshot,
    look: &BarLook,
    paused: &PausedLook,
) -> usize {
    let mut drawn = 0;
    for &(id, construction) in snap.constructions() {
        let Some((left, right)) = front_edge(frame, height, snap, id) else {
            continue;
        };
        let Some(rects) = bar_rects(left, right, construction.permille(), look, frame.ui_scale)
        else {
            continue;
        };
        let waits = snap.stalled().contains(&id);
        let (back, fill) = if waits {
            let (fill, back) = crate::money_view::paused_colors(paused);
            (back, fill)
        } else {
            let (b, f) = (look.back, look.fill);
            (Color(b.0, b.1, b.2, b.3), Color(f.0, f.1, f.2, f.3))
        };
        layer.fill_quad(rects.back, back);
        if let Some(part) = rects.fill {
            layer.fill_quad(part, fill);
        }
        if waits {
            for stroke in crate::money_view::pause_strokes(&rects, paused, frame.ui_scale) {
                layer.fill_quad(stroke, fill);
            }
        }
        drawn += 1;
    }
    drawn
}

/// The screen positions of the two ends of the front (south) edge of the footprint of structure
/// `id` of `snap`, where its bars go; `None` for anything without a footprint or off the screen.
/// `height(x, z)` is the terrain height in metres. Shared with the storage bar (WP7.3).
pub(crate) fn front_edge(
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    snap: &SimSnapshot,
    id: sf_sim::EntityId,
) -> Option<([f32; 2], [f32; 2])> {
    let view = snap.get(id)?;
    let footprint = snap.stats_of(id).and_then(|s| s.footprint)?;
    // The front edge: the footprint is centred on the entity.
    let (half_w, half_h) = (
        f32::from(footprint.width) * 0.5 * METRES_PER_CELL,
        f32::from(footprint.height) * 0.5 * METRES_PER_CELL,
    );
    let centre = (
        sf_math::render_bridge::cells_to_metres(view.pos.x),
        sf_math::render_bridge::cells_to_metres(view.pos.y),
    );
    let z = centre.1 + half_h;
    let at = |x: f32| {
        project_px(
            &frame.view_proj,
            Vec3::new(x, height(x, z), z),
            frame.viewport,
        )
        .map(|(p, _)| p)
    };
    Some((at(centre.0 - half_w)?, at(centre.0 + half_w)?))
}

/// What the game draws each frame: the bars of `snap` in the look of `looks`' build-stage table.
/// Nothing without a roster (the placeholder looks have no stages). Returns the number drawn.
pub fn draw_for(
    layer: &mut Layer<'_>,
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    snap: &SimSnapshot,
    looks: &Looks,
) -> usize {
    if snap.constructions().is_empty() {
        return 0;
    }
    let Some(roster) = looks.roster() else {
        return 0;
    };
    draw_bars(
        layer,
        frame,
        height,
        snap,
        &roster.stages().bar,
        &crate::money_view::shipped_look().paused,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOK: BarLook = BarLook {
        fill: (0, 0, 0, 0),
        back: (0, 0, 0, 0),
        height_px: 6,
        min_width_px: 18,
        max_width_px: 220,
        gap_px: 4,
    };

    #[test]
    fn the_fill_is_the_progress_of_the_bar() {
        let r = bar_rects([100.0, 300.0], [200.0, 300.0], 250, &LOOK, 1.0).unwrap();
        assert_eq!(r.back[0].0, 100.0);
        assert_eq!(r.back[1].0, 200.0);
        let fill = r.fill.unwrap();
        assert_eq!((fill[0].0, fill[1].0), (100.0, 125.0));
        assert_eq!((fill[0].1, fill[2].1), (304.0, 310.0));
    }

    #[test]
    fn nothing_is_filled_at_zero_and_a_far_structure_has_no_bar() {
        assert_eq!(
            bar_rects([100.0, 0.0], [200.0, 0.0], 0, &LOOK, 1.0)
                .unwrap()
                .fill,
            None
        );
        assert_eq!(bar_rects([100.0, 0.0], [110.0, 0.0], 500, &LOOK, 1.0), None);
        // The UI scale makes the minimum bigger.
        assert_eq!(bar_rects([100.0, 0.0], [130.0, 0.0], 500, &LOOK, 2.0), None);
        assert!(bar_rects([100.0, 0.0], [130.0, 0.0], 500, &LOOK, 1.0).is_some());
    }

    #[test]
    fn a_close_up_is_capped_in_width() {
        let r = bar_rects([0.0, 0.0], [2000.0, 0.0], 500, &LOOK, 1.0).unwrap();
        assert_eq!(r.back[1].0 - r.back[0].0, 220.0);
    }
}
