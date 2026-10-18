//! What a selection looks like (WP3.3): rings, health bars and the drag
//! rectangle. Presentation only; nothing here reads or changes anything but
//! the picture.
//!
//! * **Close up (models):** a ring on the ground around each selected unit,
//!   and a box outline around a structure, sized from the model's footprint
//!   (the box the picking uses), drawn as lines on the terrain and projected
//!   through the camera, so the ring has the perspective of the ground it lies
//!   on. A health bar floats over the model.
//! * **Strategic (icons):** the icon of a selected unit is highlighted by the
//!   icon shader (`Instancer::mark_selected` sets the flag; a white outline
//!   drawn outside the icon, clearly thicker than the thin white hover
//!   outline, yellow while a selected icon is hovered; the owner's colour
//!   inside keeps its size), and the health bar sits above the icon. In the cross-fade the ring is drawn as long as the model
//!   is, and the bar goes with the representation that is more visible.
//! * **The drag rectangle** is a translucent fill with an outline.
//!
//! All of it is drawn on the **world layer** of `sf_ui` ([`Ui::world_layer`](sf_ui::Ui::world_layer)):
//! one list of coloured quads, uploaded with the rest of the UI and drawn in
//! one draw call under the panels, however many units are selected. The
//! scratch buffer is reused, so a frame with a selection does not allocate
//! once it has been seen at its largest; with nothing selected the pass is
//! skipped altogether and the bench scene pays nothing.

use std::f32::consts::TAU;

use sf_render::glam::{Mat4, Vec3};
use sf_render::icons::Dominant;
use sf_render::pick::project_px;
use sf_render::{Blend, DrawnUnit, Instancer, ScreenRect, UnitId};
use sf_sim::{EntityId, UnitClass, World};
use sf_ui::{Color, Layer, Rect};

use crate::sandbox::NEUTRAL;
use crate::selection::{Selection, facts};

/// Segments of a unit's ring.
const RING_SEGMENTS: usize = 28;
/// A unit's ring is this much wider than the footprint of its model.
const RING_GROW: f32 = 1.25;
/// Metres a structure's outline stands off from its footprint.
const OUTLINE_MARGIN: f32 = 0.6;
/// Line width of a ring, in logical pixels.
const RING_PX: f32 = 2.5;
/// Height of a health bar, narrowest and widest, in logical pixels.
const BAR_H_PX: f32 = 4.0;
const BAR_MIN_W_PX: f32 = 26.0;
const BAR_MAX_W_PX: f32 = 120.0;
/// Gap between a model or an icon and its bar, in logical pixels.
const BAR_GAP_PX: f32 = 6.0;

/// Own selected units.
pub const OWN: Color = Color(90, 255, 120, 255);
/// Enemy unit being inspected.
pub const ENEMY: Color = Color(255, 96, 76, 255);
/// Neutral unit being inspected.
pub const NEUTRAL_TONE: Color = Color(255, 214, 90, 255);

/// The health bar's fill: green, then amber, then red.
pub fn health_color(fraction: f32) -> Color {
    if fraction > 0.6 {
        Color(70, 220, 90, 255)
    } else if fraction > 0.3 {
        Color(235, 200, 50, 255)
    } else {
        Color(230, 60, 50, 255)
    }
}

/// The shape of a ring on the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroundShape {
    /// A circle of this radius in metres.
    Circle(f32),
    /// A rectangle `along` the facing by `across` it, in metres.
    Rect { along: f32, across: f32 },
}

/// The outline of `shape` on the ground around `centre`, turned by `steps`
/// (1/256 turns), projected to the screen into `out` (cleared first).
/// `false`, with `out` unusable, when any point is behind the camera.
pub fn ground_loop(
    view_proj: &Mat4,
    viewport: [f32; 2],
    centre: Vec3,
    steps: f32,
    shape: GroundShape,
    out: &mut Vec<[f32; 2]>,
) -> bool {
    out.clear();
    let (s, c) = (steps * (TAU / 256.0)).sin_cos();
    let mut push = |local: Vec3| {
        let p = centre + Vec3::new(local.x * c - local.z * s, 0.0, local.x * s + local.z * c);
        project_px(view_proj, p, viewport).map(|(px, _)| out.push(px))
    };
    match shape {
        GroundShape::Circle(radius) => {
            for i in 0..RING_SEGMENTS {
                let a = TAU * i as f32 / RING_SEGMENTS as f32;
                if push(Vec3::new(a.cos() * radius, 0.0, a.sin() * radius)).is_none() {
                    return false;
                }
            }
        }
        GroundShape::Rect { along, across } => {
            let (hx, hz) = (0.5 * along, 0.5 * across);
            for (x, z) in [(-hx, -hz), (hx, -hz), (hx, hz), (-hx, hz)] {
                if push(Vec3::new(x, 0.0, z)).is_none() {
                    return false;
                }
            }
        }
    }
    true
}

/// The ring a selected thing gets: a box outline for a structure, a circle
/// for anything else, from the box of its model.
pub fn ring_shape(class: Option<UnitClass>, size: [f32; 3]) -> GroundShape {
    match class {
        Some(UnitClass::Structure) => GroundShape::Rect {
            along: size[0] + 2.0 * OUTLINE_MARGIN,
            across: size[2] + 2.0 * OUTLINE_MARGIN,
        },
        _ => GroundShape::Circle(0.5 * size[0].max(size[2]) * RING_GROW),
    }
}

/// The camera and the screen of the frame, as the drawing needs them.
#[derive(Clone, Copy, Debug)]
pub struct FrameGeometry {
    pub view_proj: Mat4,
    /// Window size in physical pixels.
    pub viewport: [f32; 2],
    /// The UI scale: line widths and bars follow it.
    pub ui_scale: f32,
    /// The model/icon blend of the frame.
    pub blend: Blend,
    /// Side of an icon in physical pixels (`Instancer::icon_params`).
    pub icon_px: f32,
}

/// Draws the selection feedback. Keeps a scratch buffer between frames.
#[derive(Default)]
pub struct SelectionView {
    points: Vec<[f32; 2]>,
}

/// The entity a render handle stands for.
pub fn entity_of(id: UnitId) -> EntityId {
    EntityId::new(id.index(), id.generation())
}

impl SelectionView {
    /// Marks the selected and inspected units in the instancer's frame (their
    /// icons get the selected outline, so call it before the icons are
    /// uploaded) and draws a ring and a health bar for each on `layer`.
    /// Does nothing at all, not even a pass over the units, with nothing to
    /// mark.
    pub fn mark(
        &mut self,
        instancer: &mut Instancer,
        selection: &Selection,
        world: &World,
        frame: &FrameGeometry,
        layer: &mut Layer<'_>,
    ) {
        if selection.is_empty() && selection.inspected().is_none() {
            return;
        }
        instancer.mark_selected(
            |id| selection.is_marked(entity_of(id)),
            |unit| self.draw_unit(layer, frame, selection, world, unit),
        );
    }

    fn draw_unit(
        &mut self,
        layer: &mut Layer<'_>,
        frame: &FrameGeometry,
        selection: &Selection,
        world: &World,
        unit: DrawnUnit,
    ) {
        let id = entity_of(unit.id);
        let (Some(view), scale) = (world.get(id), frame.ui_scale) else {
            return;
        };
        let class = facts(world, id).map(|f| f.class);
        let tone = if selection.inspected() == Some(id) {
            if view.owner.0 == NEUTRAL {
                NEUTRAL_TONE
            } else {
                ENEMY
            }
        } else {
            OWN
        };

        // The ring, while the model is drawn, and its width for the bar.
        let centre = Vec3::from(unit.centre);
        let mut width = None;
        if frame.blend.models_drawn()
            && ground_loop(
                &frame.view_proj,
                frame.viewport,
                centre,
                unit.steps,
                ring_shape(class, unit.size),
                &mut self.points,
            )
        {
            layer.polyline(&self.points, true, RING_PX * scale, tone);
            let (lo, hi) = self
                .points
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                    (lo.min(p[0]), hi.max(p[0]))
                });
            width = Some(hi - lo);
        }

        // The health bar: over the model while it is the larger picture,
        // over the icon beyond.
        let full = world.stats_of(id).map_or(0, |s| s.hp);
        if full == 0 {
            return;
        }
        let fraction = (view.hp as f32 / full as f32).clamp(0.0, 1.0);
        let bar_h = (BAR_H_PX * scale).round().max(2.0);
        let gap = BAR_GAP_PX * scale;
        let (cx, bottom, bar_w) = if frame.blend.dominant() == Dominant::Model {
            let top = centre + Vec3::new(0.0, unit.size[1], 0.0);
            let Some((p, _)) = project_px(&frame.view_proj, top, frame.viewport) else {
                return;
            };
            (p[0], p[1] - gap, width.unwrap_or(0.0))
        } else {
            let Some((p, _)) = project_px(
                &frame.view_proj,
                Vec3::from(unit.icon_anchor),
                frame.viewport,
            ) else {
                return;
            };
            (p[0], p[1] - 0.5 * frame.icon_px - gap, 1.6 * frame.icon_px)
        };
        let bar_w = bar_w
            .clamp(BAR_MIN_W_PX * scale, BAR_MAX_W_PX * scale)
            .round();
        let (x, y) = ((cx - 0.5 * bar_w).round(), (bottom - bar_h).round());
        let one = scale.round().max(1.0);
        layer.fill(
            Rect::new(x - one, y - one, bar_w + 2.0 * one, bar_h + 2.0 * one),
            Color(0, 0, 0, 190),
        );
        layer.fill(Rect::new(x, y, bar_w, bar_h), Color(40, 40, 44, 255));
        layer.fill(
            Rect::new(x, y, (bar_w * fraction).round(), bar_h),
            health_color(fraction),
        );
    }
}

/// Draws the drag rectangle.
pub fn draw_drag_rect(layer: &mut Layer<'_>, rect: ScreenRect, ui_scale: f32) {
    let r = Rect::new(rect.x0, rect.y0, rect.width(), rect.height());
    layer.fill(r, Color(110, 255, 140, 34));
    layer.outline(
        r,
        (1.5 * ui_scale).round().max(1.0),
        Color(150, 255, 170, 235),
    );
}

#[cfg(test)]
mod tests;
