//! What repair and sale look like on the map (WP6.6, `docs/rules/repair_sell.md`): a repair sign
//! floating above a structure that is being repaired, and a progress bar under a structure that is
//! being sold. Presentation only: it paints what the simulation reports in the snapshot
//! ([`SimSnapshot::repairs`], [`SimSnapshot::sales`]) and decides nothing. Sizes, colours and words
//! are `assets/config/repair_look.ron` ([`sf_data::repair_look`]).
//!
//! Both hang on the front (south) edge of the footprint, as wide as that edge looks on the screen,
//! like the construction bar ([`crate::construction_view`]); a structure whose edge is too small on
//! the screen (zoomed out) gets neither. They are drawn on the UI's world layer, under the panels.

use sf_data::repair_look::{RepairLook, Rgba, SignLook};
use sf_sim::RepairStatus;
use sf_ui::{Color, Layer};

use crate::construction_view::{bar_rects, front_edge};
use crate::placement_view::GhostFrame;
use crate::sim_snapshot::SimSnapshot;

fn color(c: Rgba) -> Color {
    Color(c.0, c.1, c.2, c.3)
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> [(f32, f32); 4] {
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

/// The shipped look, read once.
pub fn shipped_look() -> &'static RepairLook {
    static LOOK: std::sync::OnceLock<RepairLook> = std::sync::OnceLock::new();
    LOOK.get_or_init(RepairLook::embedded)
}

/// The quads of a repair sign: the badge and the two bars of the plus.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SignRects {
    /// The square badge.
    pub back: [(f32, f32); 4],
    /// The horizontal and the vertical bar of the plus.
    pub plus: [[(f32, f32); 4]; 2],
}

/// Where the sign of a structure goes, given the screen positions of the two ends of the front edge
/// of its footprint, the sign's look and the UI scale. The badge is centred over the middle of the
/// edge, its lower side `lift_px` above it. `None` when the edge is narrower than the look's minimum
/// or not left to right.
pub fn sign_rects(
    left: [f32; 2],
    right: [f32; 2],
    look: &SignLook,
    ui_scale: f32,
) -> Option<SignRects> {
    let width = right[0] - left[0];
    if width < f32::from(look.min_width_px) * ui_scale {
        return None;
    }
    let size = f32::from(look.size_px) * ui_scale;
    let mid = (left[0] + right[0]) * 0.5;
    let bottom = (left[1] + right[1]) * 0.5 - f32::from(look.lift_px) * ui_scale;
    let (x0, y0) = (mid - size * 0.5, bottom - size);
    let (cx, cy) = (mid, y0 + size * 0.5);
    let arm = size * f32::from(look.plus_pct) / 100.0 * 0.5;
    let thick = (f32::from(look.plus_px) * ui_scale).min(size) * 0.5;
    Some(SignRects {
        back: rect(x0, y0, x0 + size, y0 + size),
        plus: [
            rect(cx - arm, cy - thick, cx + arm, cy + thick),
            rect(cx - thick, cy - arm, cx + thick, cy + arm),
        ],
    })
}

/// Draws the repair signs and the sale bars of `snap` on `layer`. `height(x, z)` is the terrain
/// height in metres. Returns `(signs, bars)` drawn.
pub fn draw(
    layer: &mut Layer<'_>,
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    snap: &SimSnapshot,
    look: &RepairLook,
) -> (usize, usize) {
    let (mut signs, mut bars) = (0, 0);
    for &(id, status) in snap.repairs() {
        let Some((left, right)) = front_edge(frame, height, snap, id) else {
            continue;
        };
        let Some(rects) = sign_rects(left, right, &look.sign, frame.ui_scale) else {
            continue;
        };
        let (back, plus) = match status {
            RepairStatus::Repairing => (look.sign.back, look.sign.plus),
            RepairStatus::WaitingForMoney => (look.sign.waiting_back, look.sign.waiting_plus),
        };
        layer.fill_quad(rects.back, color(back));
        for bar in rects.plus {
            layer.fill_quad(bar, color(plus));
        }
        signs += 1;
    }
    for &(id, sale) in snap.sales() {
        let Some((left, right)) = front_edge(frame, height, snap, id) else {
            continue;
        };
        let Some(rects) = bar_rects(left, right, sale.permille(), &look.sale_bar, frame.ui_scale)
        else {
            continue;
        };
        layer.fill_quad(rects.back, color(look.sale_bar.back));
        if let Some(part) = rects.fill {
            layer.fill_quad(part, color(look.sale_bar.fill));
        }
        bars += 1;
    }
    (signs, bars)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> SignLook {
        RepairLook::embedded().sign
    }

    #[test]
    fn the_sign_hangs_over_the_middle_of_the_front_edge() {
        let l = look();
        let r = sign_rects([100.0, 300.0], [200.0, 300.0], &l, 1.0).unwrap();
        // 20 px square, its lower side 30 px above the edge, centred on x = 150.
        assert_eq!(r.back, rect(140.0, 250.0, 160.0, 270.0));
        // The plus: 64 % of 20 = 12.8 long, 4 thick, centred on (150, 260).
        let h = r.plus[0];
        assert_eq!((h[0].0, h[1].0), (143.6, 156.4));
        assert_eq!((h[0].1, h[2].1), (258.0, 262.0));
        let v = r.plus[1];
        assert_eq!((v[0].0, v[1].0), (148.0, 152.0));
        assert_eq!((v[0].1, v[2].1), (253.6, 266.4));
    }

    #[test]
    fn the_sign_grows_with_the_ui_scale_and_a_far_structure_has_none() {
        let l = look();
        let r = sign_rects([100.0, 300.0], [200.0, 300.0], &l, 2.0).unwrap();
        assert_eq!(r.back[1].0 - r.back[0].0, 40.0);
        assert_eq!(sign_rects([100.0, 0.0], [110.0, 0.0], &l, 1.0), None);
        // The minimum width grows with the scale too.
        assert_eq!(sign_rects([100.0, 0.0], [130.0, 0.0], &l, 2.0), None);
        assert!(sign_rects([100.0, 0.0], [130.0, 0.0], &l, 1.0).is_some());
    }
}
