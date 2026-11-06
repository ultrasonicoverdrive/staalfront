//! The debug view of shroud and fog of war (WP8.1, `docs/rules/vision.md`): the cells the local
//! player has never seen darkened, the ones it saw before but does not see now dimmed. Presentation
//! only; it paints what the snapshot carries ([`SimSnapshot::visibility`]) and decides nothing.
//!
//! **Debug only.** It is a coloured film over the picture, and the units under it are not hidden: a
//! unit of the other side in the shroud shows through the film. Hiding what the player has not seen
//! (units, ghosts of structures, soft edges, the icon list, the minimap) is WP8.3 and WP8.4. The
//! switch is the "Fog of war (debug)" checkbox of the F1 panel or `--debug-fog`; colours are
//! `assets/config/vision_look.ron`.
//!
//! Cells are drawn as quads in runs along a row, at most `run_cells` long, projected onto the
//! terrain through the camera like the placement ghost ([`crate::placement_view`]), on the UI's
//! world layer, under the panels.

use sf_data::vision_look::{Rgba, VisionLook};
use sf_math::render_bridge::METRES_PER_CELL;
use sf_render::glam::{Mat4, Vec3};
use sf_render::pick::project_px;
use sf_sim::{Cell, Visibility};
use sf_ui::{Color, Layer};

use crate::placement_view::GhostFrame;
use crate::sim_snapshot::SimSnapshot;

/// Metres above the terrain the quads lie, so they do not sink into a slope.
const LIFT: f32 = 0.2;

/// The shipped look, read once.
pub fn shipped_look() -> &'static VisionLook {
    static LOOK: std::sync::OnceLock<VisionLook> = std::sync::OnceLock::new();
    LOOK.get_or_init(VisionLook::embedded)
}

fn color(c: Rgba) -> Color {
    Color(c.0, c.1, c.2, c.3)
}

/// A stretch of cells along row `y`, `x0` to `x1` inclusive, all in the state `state`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// The row.
    pub y: u32,
    /// The first column.
    pub x0: u32,
    /// The last column.
    pub x1: u32,
    /// [`Visibility::Shroud`] or [`Visibility::Fog`]; visible cells are not drawn.
    pub state: Visibility,
}

/// The runs to draw for a `width` by `height` map: the cells that are not visible, merged along a
/// row while the state stays the same, at most `max_run` cells long.
pub fn runs(width: u32, height: u32, max_run: u32, state: &dyn Fn(Cell) -> Visibility) -> Vec<Run> {
    let max_run = max_run.max(1);
    let mut out: Vec<Run> = Vec::new();
    for y in 0..height {
        let mut open: Option<Run> = None;
        for x in 0..width {
            let s = state(Cell::new(x, y));
            if s == Visibility::Visible {
                out.extend(open.take());
                continue;
            }
            match &mut open {
                Some(r) if r.state == s && x - r.x0 < max_run => r.x1 = x,
                _ => {
                    out.extend(open.take());
                    open = Some(Run {
                        y,
                        x0: x,
                        x1: x,
                        state: s,
                    });
                }
            }
        }
        out.extend(open.take());
    }
    out
}

fn project(
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    x: u32,
    y: u32,
) -> Option<(f32, f32)> {
    let (mx, mz) = (x as f32 * METRES_PER_CELL, y as f32 * METRES_PER_CELL);
    let p = Vec3::new(mx, height(mx, mz) + LIFT, mz);
    let view_proj: &Mat4 = &frame.view_proj;
    project_px(view_proj, p, frame.viewport).map(|(px, _)| (px[0], px[1]))
}

/// Whether a projected quad is worth drawing: some of it can be on the screen, and no corner is so far
/// out that the quad (it crosses the near plane of the camera) would smear across the picture.
pub fn worth_drawing(corners: &[(f32, f32); 4], viewport: [f32; 2]) -> bool {
    let reach = 3.0;
    let far = |(x, y): (f32, f32)| x.abs() > viewport[0] * reach || y.abs() > viewport[1] * reach;
    if corners.iter().any(|&c| far(c)) {
        return false;
    }
    let (min_x, max_x) = corners.iter().fold((f32::MAX, f32::MIN), |(lo, hi), c| {
        (lo.min(c.0), hi.max(c.0))
    });
    let (min_y, max_y) = corners.iter().fold((f32::MAX, f32::MIN), |(lo, hi), c| {
        (lo.min(c.1), hi.max(c.1))
    });
    max_x >= 0.0 && min_x <= viewport[0] && max_y >= 0.0 && min_y <= viewport[1]
}

/// Draws the film over `snap`'s map on `layer`. `height(x, z)` is the terrain height in metres.
/// Returns the number of quads drawn; nothing in a world without vision rules.
pub fn draw_debug(
    layer: &mut Layer<'_>,
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    snap: &SimSnapshot,
    look: &VisionLook,
) -> usize {
    if snap.fog_version().is_none() {
        return 0;
    }
    let (w, h) = snap.map_size();
    let mut drawn = 0;
    for r in runs(w, h, look.run_cells, &|c| snap.visibility(c)) {
        // The four corners of the run, clockwise from the north-west one.
        let corners = [
            project(frame, height, r.x0, r.y),
            project(frame, height, r.x1 + 1, r.y),
            project(frame, height, r.x1 + 1, r.y + 1),
            project(frame, height, r.x0, r.y + 1),
        ];
        let [Some(a), Some(b), Some(c), Some(d)] = corners else {
            continue;
        };
        if !worth_drawing(&[a, b, c, d], frame.viewport) {
            continue;
        }
        let tone = match r.state {
            Visibility::Shroud => look.shroud,
            _ => look.fog,
        };
        layer.fill_quad([a, b, c, d], color(tone));
        drawn += 1;
    }
    drawn
}

#[cfg(test)]
mod tests;
