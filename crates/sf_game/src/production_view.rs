//! Production in the sandbox (WP6.5): the progress bar under a factory that builds, the queue
//! readout of the selected factory and the rally flag. Presentation only: the queues, the progress
//! and the rally points are the simulation's ([`SimSnapshot::factories`]); nothing here decides
//! anything. The finished game's sidebar is WP11.1.
//!
//! The bar lies where the construction bar lies (under the front edge of the footprint) and is
//! drawn in the waiting colour while the factory waits for money or for a free spot. The readout
//! is a panel of lines on the left edge: the factory, its queue (the front item with its
//! progress) and the rally cell. Every number is `assets/config/production_look.ron`
//! ([`sf_data::production_look`]).

use sf_data::production_look::ProductionLook;
use sf_math::render_bridge::METRES_PER_CELL;
use sf_render::glam::Vec3;
use sf_render::pick::project_px;
use sf_sim::{BuildingFunction, EntityId, FactoryStatus, FactoryView, Kind};
use sf_ui::{Anchor, Color, Layer, Ui};

use crate::construction_view::{bar_rects, front_edge};
use crate::placement_view::GhostFrame;
use crate::selection::Selection;
use crate::sim_snapshot::SimSnapshot;

/// The shipped look, read once.
pub fn shipped_look() -> &'static ProductionLook {
    static LOOK: std::sync::OnceLock<ProductionLook> = std::sync::OnceLock::new();
    LOOK.get_or_init(ProductionLook::embedded)
}

fn color(c: (u8, u8, u8, u8)) -> Color {
    Color(c.0, c.1, c.2, c.3)
}

/// The selected own factory (the lowest id if several are selected) and its function; `None`
/// when no factory is selected.
pub fn selected_factory(
    snap: &SimSnapshot,
    selection: &Selection,
) -> Option<(EntityId, BuildingFunction)> {
    selection.ids().iter().find_map(|&id| {
        let kind = snap.get(id)?.kind;
        Some((id, snap.menu().factory_function(kind)?))
    })
}

/// Whether the bar of the factory is drawn in the waiting colour: it is not gaining progress.
pub fn bar_waits(status: FactoryStatus) -> bool {
    !matches!(status, FactoryStatus::Building)
}

/// Draws the bar of every factory that has something in its queue that takes time, on `layer`.
/// `height(x, z)` is the terrain height in metres. Returns the number of bars drawn.
pub fn draw_bars(
    layer: &mut Layer<'_>,
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    snap: &SimSnapshot,
    look: &ProductionLook,
) -> usize {
    let mut drawn = 0;
    for (id, view) in snap.factories() {
        if view.queue.is_empty() || view.front_ticks == 0 {
            continue;
        }
        let Some((left, right)) = front_edge(frame, height, snap, *id) else {
            continue;
        };
        let Some(rects) = bar_rects(left, right, view.front_permille, &look.bar, frame.ui_scale)
        else {
            continue;
        };
        let b = look.bar.back;
        layer.fill_quad(rects.back, Color(b.0, b.1, b.2, b.3));
        if let Some(fill) = rects.fill {
            let c = if bar_waits(view.status) {
                look.waiting
            } else {
                look.bar.fill
            };
            layer.fill_quad(fill, color(c));
        }
        drawn += 1;
    }
    drawn
}

/// The corners of the pole and of the flag of a rally flag standing at screen position `foot`
/// (the middle of the cell), as quads (clockwise from top left).
pub fn flag_quads(
    foot: [f32; 2],
    look: &sf_data::production_look::RallyLook,
    ui_scale: f32,
) -> ([(f32, f32); 4], [(f32, f32); 4]) {
    let (pw, ph) = (
        f32::from(look.pole_width_px) * ui_scale,
        f32::from(look.pole_height_px) * ui_scale,
    );
    let (fw, fh) = (
        f32::from(look.flag_width_px) * ui_scale,
        f32::from(look.flag_height_px) * ui_scale,
    );
    let (x0, y0, y1) = (foot[0] - pw * 0.5, foot[1] - ph, foot[1]);
    let pole = [(x0, y0), (x0 + pw, y0), (x0 + pw, y1), (x0, y1)];
    // The flag hangs from the top of the pole, to the right.
    let fx = x0 + pw;
    let flag = [(fx, y0), (fx + fw, y0), (fx + fw, y0 + fh), (fx, y0 + fh)];
    (pole, flag)
}

/// Draws the rally flag of every factory that has a rally point; the one of `selected` in its own
/// colour. Returns the number of flags drawn.
pub fn draw_rally_flags(
    layer: &mut Layer<'_>,
    frame: &GhostFrame,
    height: &dyn Fn(f32, f32) -> f32,
    snap: &SimSnapshot,
    selected: Option<EntityId>,
    look: &ProductionLook,
) -> usize {
    let mut drawn = 0;
    for (id, view) in snap.factories() {
        let Some(cell) = view.rally else { continue };
        let x = (cell.x as f32 + 0.5) * METRES_PER_CELL;
        let z = (cell.y as f32 + 0.5) * METRES_PER_CELL;
        let Some((foot, _)) = project_px(
            &frame.view_proj,
            Vec3::new(x, height(x, z), z),
            frame.viewport,
        ) else {
            continue;
        };
        let (pole, flag) = flag_quads(foot, &look.rally, frame.ui_scale);
        layer.fill_quad(pole, color(look.rally.pole));
        let c = if selected == Some(*id) {
            look.rally.flag_selected
        } else {
            look.rally.flag
        };
        layer.fill_quad(flag, color(c));
        drawn += 1;
    }
    drawn
}

/// How one line of the readout is coloured.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Accent,
    Warn,
}

/// The words for what a factory is doing; `name` turns a kind into its name.
pub fn status_words(status: FactoryStatus, name: &dyn Fn(Kind) -> String) -> String {
    match status {
        FactoryStatus::Idle => "idle".to_owned(),
        FactoryStatus::Building => "building".to_owned(),
        FactoryStatus::WaitingForMoney => "waiting for credits".to_owned(),
        FactoryStatus::WaitingForExit => "exit blocked, holding the queue".to_owned(),
        FactoryStatus::MissingPrerequisite(k) => format!("needs a {} first", name(k)),
        FactoryStatus::Stopped => "stopped".to_owned(),
        FactoryStatus::NotStanding => "not standing".to_owned(),
    }
}

/// The lines of the readout for the factory `title` with `view`: the status, one line per queue
/// item (the front item with its progress and the credits paid) and the rally cell; and last the
/// unit the queue key would add (`next`, with its price and build ticks).
pub fn panel_lines(
    title: &str,
    view: Option<&FactoryView>,
    next: Option<(&str, u32, u32)>,
    name: &dyn Fn(Kind) -> String,
) -> Vec<(String, Tone)> {
    let mut lines = Vec::new();
    match view {
        None => lines.push((format!("{title}: idle, rally not set"), Tone::Plain)),
        Some(v) => {
            let tone = if bar_waits(v.status) && !v.queue.is_empty() {
                Tone::Warn
            } else {
                Tone::Plain
            };
            lines.push((format!("{title}: {}", status_words(v.status, name)), tone));
            for (n, item) in v.queue.iter().enumerate() {
                let label = name(item.kind);
                if n == 0 && v.front_ticks > 0 {
                    lines.push((
                        format!(
                            "1. {label}  {}%  ({} credits paid)",
                            v.front_permille / 10,
                            v.front_paid
                        ),
                        Tone::Accent,
                    ));
                } else {
                    lines.push((format!("{}. {label}", n + 1), Tone::Plain));
                }
            }
            lines.push((
                match v.rally {
                    Some(c) => format!("rally: {}", c),
                    None => "rally: not set".to_owned(),
                },
                Tone::Plain,
            ));
        }
    }
    if let Some((label, cost, ticks)) = next {
        lines.push((
            format!("next: {label} ({cost} credits, {ticks} ticks)"),
            Tone::Plain,
        ));
    }
    lines
}

/// Draws the readout lines on the left edge of `ui`.
pub fn draw_panel(ui: &mut Ui, lines: &[(String, Tone)], look: &ProductionLook) {
    let p = &look.panel;
    let scale = ui.scale();
    let px = f32::from(p.text_px) * scale;
    let size = (f32::from(p.width_px), f32::from(p.line_px));
    for (n, (text, tone)) in lines.iter().enumerate() {
        let top = f32::from(p.top_px) + n as f32 * (size.1 + 2.0);
        let rect = ui.panel(Anchor::TopLeft, (f32::from(p.margin_px), top), size);
        let (_, text_h) = ui.measure_px(text, px);
        let c = match tone {
            Tone::Plain => look.text,
            Tone::Accent => look.accent,
            Tone::Warn => look.warn,
        };
        ui.text_px_at(
            rect.x + f32::from(p.pad_px) * scale,
            rect.y + (rect.h - text_h) * 0.5,
            text,
            px,
            color(c),
        );
    }
}

/// The unit a queue key would add: `menu[index % len]` of the units the factory builds for the
/// player's faction. `None` for an empty menu.
pub fn menu_pick(menu: &[Kind], index: usize) -> Option<Kind> {
    (!menu.is_empty()).then(|| menu[index % menu.len()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_sim::{Cell, QueueItem};

    fn name(k: Kind) -> String {
        format!("kind{}", k.0)
    }

    fn view(status: FactoryStatus) -> FactoryView {
        FactoryView {
            queue: vec![
                QueueItem {
                    kind: Kind(7),
                    free: false,
                },
                QueueItem {
                    kind: Kind(9),
                    free: false,
                },
            ],
            front_permille: 425,
            front_ticks: 80,
            status,
            rally: Some(Cell::new(31, 12)),
            front_paid: 85,
        }
    }

    #[test]
    fn the_readout_names_every_queue_item_and_the_front_progress() {
        let lines = panel_lines(
            "barracks",
            Some(&view(FactoryStatus::Building)),
            Some(("rifle", 100, 60)),
            &name,
        );
        let texts: Vec<&str> = lines.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            texts,
            [
                "barracks: building",
                "1. kind7  42%  (85 credits paid)",
                "2. kind9",
                "rally: (31, 12)",
                "next: rifle (100 credits, 60 ticks)"
            ]
        );
        assert_eq!(lines[1].1, Tone::Accent);
        assert_eq!(lines[0].1, Tone::Plain);
    }

    #[test]
    fn a_waiting_factory_is_in_the_warning_tone_and_says_why() {
        let l = panel_lines(
            "factory",
            Some(&view(FactoryStatus::WaitingForMoney)),
            None,
            &name,
        );
        assert_eq!(
            l[0],
            ("factory: waiting for credits".to_owned(), Tone::Warn)
        );
        let l = panel_lines(
            "factory",
            Some(&view(FactoryStatus::WaitingForExit)),
            None,
            &name,
        );
        assert!(l[0].0.contains("exit blocked"));
        assert!(bar_waits(FactoryStatus::WaitingForExit));
        assert!(bar_waits(FactoryStatus::WaitingForMoney));
        assert!(!bar_waits(FactoryStatus::Building));
    }

    #[test]
    fn a_missing_prerequisite_is_named() {
        assert_eq!(
            status_words(FactoryStatus::MissingPrerequisite(Kind(4)), &name),
            "needs a kind4 first"
        );
    }

    #[test]
    fn a_factory_without_a_view_reads_idle() {
        let l = panel_lines("barracks", None, None, &name);
        assert_eq!(l[0].0, "barracks: idle, rally not set");
    }

    #[test]
    fn the_flag_hangs_from_the_top_of_the_pole_on_the_foot() {
        let look = ProductionLook::embedded().rally;
        let (pole, flag) = flag_quads([100.0, 200.0], &look, 1.0);
        assert_eq!(pole[2].1, 200.0);
        assert_eq!(pole[0].1, 200.0 - f32::from(look.pole_height_px));
        assert_eq!(flag[0].1, pole[0].1);
        assert_eq!(flag[0].0, pole[1].0);
        assert_eq!(flag[1].0 - flag[0].0, f32::from(look.flag_width_px));
        assert_eq!(flag[2].1 - flag[0].1, f32::from(look.flag_height_px));
        // The UI scale scales it.
        let (_, big) = flag_quads([100.0, 200.0], &look, 2.0);
        assert_eq!(big[1].0 - big[0].0, 2.0 * f32::from(look.flag_width_px));
    }

    #[test]
    fn the_menu_pick_wraps_and_an_empty_menu_picks_nothing() {
        let m = [Kind(3), Kind(5), Kind(8)];
        assert_eq!(menu_pick(&m, 0), Some(Kind(3)));
        assert_eq!(menu_pick(&m, 4), Some(Kind(5)));
        assert_eq!(menu_pick(&[], 2), None);
    }
}
