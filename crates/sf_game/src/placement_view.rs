//! The ghost of the structure being placed (WP6.2): the footprint under the cursor,
//! snapped to cells and tinted cell by cell, the exit cell of a factory, and the words for
//! why a place is refused. Presentation only.
//!
//! **There are no rules here.** The sim thread asks the world the question the ghost poses
//! ([`GhostQuery`], answered by `World::preview_placement`, the function the `BuildStructure`
//! command runs) and the answer comes back in the snapshot ([`GhostAnswer`]). This module paints
//! that answer, so the picture and the command cannot disagree
//! (`the_preview_and_the_command_never_disagree` in `sf_sim`, `the_ghost_is_the_commands_verdict`
//! here).
//!
//! # What the tints mean
//!
//! | Tint | Means |
//! |---|---|
//! | green | the placement stands, this cell too |
//! | red | this cell is the problem: off the map, terrain that cannot be built on, a structure or a unit in it |
//! | amber | this cell is fine, but the placement is refused for another reason: too far from your base, a prerequisite missing, not your faction's structure, the exit blocked |
//!
//! A cell that the sim calls a structure or a unit, but that the player cannot see now, is drawn in
//! a neutral colour (`placement_unknown` of `vision_look.ron`) and not red: the preview must not show
//! what is hidden (`docs/rules/fog_look.md`). The verdict itself is the sim's and is not changed; a
//! placement it refuses is still drawn as refused (outline, amber cells).
//!
//! The outline round the footprint is green or red by the verdict. A factory's exit cell is drawn
//! blue when units can come out of it and red when not.
//!
//! The quads are projected onto the terrain through the camera like the selection rings
//! ([`crate::selection_view`]) and go on the UI's world layer, under the panels.

use sf_math::render_bridge::METRES_PER_CELL;
use sf_render::glam::{Mat4, Vec3};
use sf_render::pick::project_px;
use sf_sim::{Cell, CellVerdict, Kind, PlacementError, PlacementPreview};
use sf_ui::{Color, Layer};

use crate::sim_snapshot::GhostAnswer;

/// The camera and the screen of the frame, as far as the ghost needs them.
#[derive(Clone, Copy, Debug)]
pub struct GhostFrame {
    /// World to clip space.
    pub view_proj: Mat4,
    /// Window size in physical pixels.
    pub viewport: [f32; 2],
    /// The UI scale: line widths follow it.
    pub ui_scale: f32,
}

/// A cell of a placement that stands.
pub const VALID: Color = Color(70, 220, 90, 120);
/// A cell that is the reason the placement is refused.
pub const BLOCKED: Color = Color(230, 60, 50, 150);
/// A cell that is fine in a placement refused for another reason.
pub const ELSEWHERE: Color = Color(240, 190, 60, 120);
/// The outline of a placement that stands.
pub const OUTLINE_VALID: Color = Color(120, 255, 140, 235);
/// The outline of a placement that is refused.
pub const OUTLINE_REFUSED: Color = Color(255, 110, 90, 235);
/// The exit cell units can come out of.
pub const EXIT_OPEN: Color = Color(90, 170, 255, 150);
/// The exit cell that is blocked.
pub const EXIT_BLOCKED: Color = Color(230, 60, 50, 150);
/// The outlines of the two exit cells.
const EXIT_OPEN_LINE: Color = Color(130, 190, 255, 235);
const EXIT_BLOCKED_LINE: Color = Color(255, 110, 90, 235);

/// Metres above the terrain the quads are drawn at, so they do not z-fight with it in the
/// picture's own depth (the layer has none, this is for the perspective of a slope).
const LIFT: f32 = 0.15;
/// How much of a cell each tinted quad is short of its edge, so that the cells of a footprint
/// read as cells.
const CELL_INSET: f32 = 0.05;
/// Line width of the outline, logical pixels.
const OUTLINE_PX: f32 = 2.5;

/// How one cell of the ghost is tinted.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tint {
    /// Green.
    Valid,
    /// Red.
    Blocked,
    /// Amber.
    Elsewhere,
    /// Grey: the cell holds something the player cannot see.
    Unknown,
}

/// The tint of a cell with `verdict` in a placement that is (`placement_ok`) or is not allowed.
pub fn tint_of(verdict: CellVerdict, placement_ok: bool) -> Tint {
    match (verdict, placement_ok) {
        (CellVerdict::Free, true) => Tint::Valid,
        (CellVerdict::Free, false) => Tint::Elsewhere,
        _ => Tint::Blocked,
    }
}

/// [`tint_of`] for what the player can see: every cell the player cannot see now is `Unknown`,
/// whether the sim calls it free, a structure or a unit, so grey means "not in sight" and never
/// "something hidden stands here". That also covers an own structure outside own sight.
/// Terrain and the map edge are not hidden.
pub fn tint_seen(verdict: CellVerdict, placement_ok: bool, seen: bool) -> Tint {
    match verdict {
        CellVerdict::Free | CellVerdict::Structure | CellVerdict::Unit if !seen => Tint::Unknown,
        _ => tint_of(verdict, placement_ok),
    }
}

/// The tint of every cell of `preview` that is on the map, as [`draw_ghost`] paints them.
pub fn tints(preview: &PlacementPreview, seen: &dyn Fn(Cell) -> bool) -> Vec<(Cell, Tint)> {
    let ok = preview.verdict.is_ok();
    preview
        .cells
        .iter()
        // A cell off the map has no ground to lie on.
        .filter(|&&(_, verdict)| verdict != CellVerdict::OffMap)
        .map(|&(cell, verdict)| (cell, tint_seen(verdict, ok, seen(cell))))
        .collect()
}

fn color_of(tint: Tint) -> Color {
    match tint {
        Tint::Valid => VALID,
        Tint::Blocked => BLOCKED,
        Tint::Elsewhere => ELSEWHERE,
        Tint::Unknown => {
            let c = crate::vision_view::shipped_look().placement_unknown;
            Color(c.0, c.1, c.2, c.3)
        }
    }
}

/// The words for the refusal of `preview`: [`describe`], except that "another structure" or "a unit
/// is in the way" is only said when `seen` shows a cell with that very thing; otherwise the player
/// is not told what is hidden and gets a plain "cannot build here".
pub fn refusal_words(
    preview: &PlacementPreview,
    seen: &dyn Fn(Cell) -> bool,
    name_of: &dyn Fn(Kind) -> String,
) -> String {
    let Err(error) = preview.verdict else {
        return "place here".into();
    };
    let culprit = match error {
        PlacementError::Occupied => Some(CellVerdict::Structure),
        PlacementError::UnitInTheWay => Some(CellVerdict::Unit),
        _ => None,
    };
    if let Some(culprit) = culprit
        && !preview
            .cells
            .iter()
            .any(|&(cell, v)| v == culprit && seen(cell))
    {
        return "cannot build here".into();
    }
    describe(error, name_of)
}

/// The words for a refusal the sim gave on a click, for a structure of `size` (cells along x, y)
/// anchored at `anchor`: "another structure / a unit is in the way" only when every footprint cell
/// is in sight, else the same generic "cannot build here" as [`refusal_words`].
pub fn click_refusal_words(
    error: PlacementError,
    anchor: Cell,
    size: (u8, u8),
    seen: &dyn Fn(Cell) -> bool,
    name_of: &dyn Fn(Kind) -> String,
) -> String {
    let hides = matches!(
        error,
        PlacementError::Occupied | PlacementError::UnitInTheWay
    );
    let (w, h) = (u32::from(size.0), u32::from(size.1));
    let all_seen = (anchor.y..anchor.y.saturating_add(h))
        .all(|y| (anchor.x..anchor.x.saturating_add(w)).all(|x| seen(Cell::new(x, y))));
    if hides && !all_seen {
        return "cannot build here".into();
    }
    describe(error, name_of)
}

/// The words for a refusal. `name_of` names a structure kind (for "needs a power plant first").
pub fn describe(error: PlacementError, name_of: &dyn Fn(Kind) -> String) -> String {
    match error {
        PlacementError::NoBuildingRules => "no building rules in this game".into(),
        PlacementError::UnknownKind => "not a structure".into(),
        PlacementError::CannotBeBuilt => "this cannot be built".into(),
        PlacementError::NoFaction => "you have no faction".into(),
        PlacementError::WrongFaction => "your faction does not build this".into(),
        PlacementError::MissingPrerequisite(kind) => {
            format!("needs {} first", name_of(kind))
        }
        PlacementError::OffMap => "outside the map".into(),
        PlacementError::BadTerrain => "cannot build on this ground".into(),
        PlacementError::Occupied => "another structure is in the way".into(),
        PlacementError::UnitInTheWay => "a unit is in the way".into(),
        PlacementError::TooFarFromBase => "too far from your base".into(),
        PlacementError::ExitBlocked => "the exit is blocked".into(),
        PlacementError::EntityLimit => "too many things in the world".into(),
    }
}

/// The corner of a cell in metres: `(x, z)` of the corner `(dx, dy)` of `cell`.
fn corner(cell: Cell, dx: u32, dy: u32) -> (f32, f32) {
    (
        (cell.x + dx) as f32 * METRES_PER_CELL,
        (cell.y + dy) as f32 * METRES_PER_CELL,
    )
}

/// The screen position of a corner on the terrain, `None` when it is behind the camera.
fn project(
    view_proj: &Mat4,
    viewport: [f32; 2],
    height: &dyn Fn(f32, f32) -> f32,
    (x, z): (f32, f32),
) -> Option<(f32, f32)> {
    let p = Vec3::new(x, height(x, z) + LIFT, z);
    project_px(view_proj, p, viewport).map(|(px, _)| (px[0], px[1]))
}

/// The four screen corners of `cell`, clockwise from the north-west one, each pulled `inset` of
/// a cell towards the middle so that a seam shows between two cells.
fn cell_quad(
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    cell: Cell,
    inset: f32,
) -> Option<[(f32, f32); 4]> {
    let (x0, z0) = corner(cell, 0, 0);
    let at = |fx: f32, fy: f32| {
        let along =
            |origin: f32, f: f32| origin + (inset + f * (1.0 - 2.0 * inset)) * METRES_PER_CELL;
        project(
            &frame.view_proj,
            frame.viewport,
            height,
            (along(x0, fx), along(z0, fy)),
        )
    };
    Some([at(0.0, 0.0)?, at(1.0, 0.0)?, at(1.0, 1.0)?, at(0.0, 1.0)?])
}

/// Draws the ghost of `answer` on `layer`: a tinted quad for each cell, the outline of the
/// footprint and the exit cell. `height(x, z)` is the terrain height in metres. Returns the
/// number of cells drawn (a cell behind the camera is skipped). `seen(cell)` says whether the player
/// sees the cell now (always true in a world without vision rules).
pub fn draw_ghost(
    layer: &mut Layer<'_>,
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    answer: &GhostAnswer,
    seen: &dyn Fn(Cell) -> bool,
) -> usize {
    let preview = &answer.preview;
    let ok = preview.verdict.is_ok();
    let mut drawn = 0;
    for (cell, tint) in tints(preview, seen) {
        if let Some(quad) = cell_quad(frame, height, cell, CELL_INSET) {
            layer.fill_quad(quad, color_of(tint));
            drawn += 1;
        }
    }
    // The outline of the whole footprint.
    if let Some(fp) = preview.footprint {
        let origin = answer.query.cell;
        let (w, h) = (u32::from(fp.width), u32::from(fp.height));
        let at = |dx, dy| {
            project(
                &frame.view_proj,
                frame.viewport,
                height,
                corner(origin, dx, dy),
            )
        };
        if let [Some(a), Some(b), Some(c), Some(d)] = [at(0, 0), at(w, 0), at(w, h), at(0, h)] {
            let points = [[a.0, a.1], [b.0, b.1], [c.0, c.1], [d.0, d.1]];
            let tone = if ok { OUTLINE_VALID } else { OUTLINE_REFUSED };
            layer.polyline(&points, true, OUTLINE_PX * frame.ui_scale, tone);
        }
    }
    if let Some(exit) = preview.exit
        && let Some(quad) = cell_quad(frame, height, exit.cell, 0.0)
    {
        let (fill, line) = if exit.usable {
            (EXIT_OPEN, EXIT_OPEN_LINE)
        } else {
            (EXIT_BLOCKED, EXIT_BLOCKED_LINE)
        };
        layer.fill_quad(quad, fill);
        let points = quad.map(|(x, y)| [x, y]);
        layer.polyline(&points, true, OUTLINE_PX * frame.ui_scale, line);
    }
    drawn
}

#[cfg(test)]
mod tests;
