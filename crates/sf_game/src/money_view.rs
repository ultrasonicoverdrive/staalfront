//! The money in the player HUD (WP7.4). Presentation only: the credits and the structures that
//! wait for them are the simulation's (`World::credits`, `World::stalled`), copied into every
//! snapshot ([`SimSnapshot::credits_of`], [`SimSnapshot::stalled`]); nothing here decides anything.
//!
//! Two things are shown. A **readout** in the top right corner, "Credits 1000", in the warning
//! colour when the credits are few or a structure of the player waits, with a strip under it
//! ("build paused: no credits") while one waits. And the **progress bar** of a structure that
//! waits for money: another fill and back colour and two pause strokes at its right end
//! ([`pause_strokes`]). Every number and word is `assets/config/money_look.ron`
//! ([`sf_data::money_look`]).

use sf_data::money_look::{MoneyLook, PausedLook};
use sf_ui::{Anchor, Color, Ui};

use crate::construction_view::BarRects;

/// The shipped look, read once.
pub fn shipped_look() -> &'static MoneyLook {
    static LOOK: std::sync::OnceLock<MoneyLook> = std::sync::OnceLock::new();
    LOOK.get_or_init(MoneyLook::embedded)
}

/// What the HUD shows of the money of the local player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoneyHud {
    /// The credits the player holds.
    pub credits: u64,
    /// Whether a structure of the player waits for money.
    pub waiting: bool,
}

/// The text of the readout: the label and the number.
pub fn readout_text(look: &MoneyLook, credits: u64) -> String {
    format!("{} {}", look.label, credits)
}

/// The colour of the readout: the warning colour when the credits are few or a build waits.
pub fn readout_color(look: &MoneyLook, credits: u64, waiting: bool) -> Color {
    let c = if waiting || credits < look.low_below {
        look.low
    } else {
        look.text
    };
    Color(c.0, c.1, c.2, c.3)
}

/// Draws the readout (and, while a build waits, the strip under it) on `ui`. Call it while the
/// UI of the frame is being built, after the other panels.
pub fn draw_panel(ui: &mut Ui, money: &MoneyHud, look: &MoneyLook) {
    let p = &look.panel;
    let size = (f32::from(p.width_px), f32::from(p.height_px));
    let margin = f32::from(p.margin_px);
    let scale = ui.scale();
    let px = f32::from(p.text_px) * scale;
    let color = readout_color(look, money.credits, money.waiting);
    let line = |ui: &mut Ui, offset_y: f32, text: &str| {
        let rect = ui.panel(Anchor::TopRight, (margin, margin + offset_y), size);
        let (_, text_h) = ui.measure_px(text, px);
        ui.text_px_at(
            rect.x + f32::from(p.pad_px) * scale,
            rect.y + (rect.h - text_h) * 0.5,
            text,
            px,
            color,
        );
    };
    line(ui, 0.0, &readout_text(look, money.credits));
    if money.waiting {
        line(ui, size.1 + f32::from(p.gap_px), &look.waiting);
    }
}

/// The colours of the bar of a structure that waits for money, as [`Color`]s: fill and back.
pub fn paused_colors(look: &PausedLook) -> (Color, Color) {
    let c = |c: (u8, u8, u8, u8)| Color(c.0, c.1, c.2, c.3);
    (c(look.fill), c(look.back))
}

/// The two pause strokes at the right end of the bar `rects`: two vertical quads side by side,
/// each as wide as `stroke_width_px`, `stroke_gap_px` apart, standing out `stroke_extra_px`
/// above and below the bar. The left stroke ends where the bar ends.
pub fn pause_strokes(rects: &BarRects, look: &PausedLook, ui_scale: f32) -> [[(f32, f32); 4]; 2] {
    let (right, top, bottom) = (rects.back[1].0, rects.back[0].1, rects.back[2].1);
    let extra = f32::from(look.stroke_extra_px) * ui_scale;
    let width = f32::from(look.stroke_width_px) * ui_scale;
    let gap = f32::from(look.stroke_gap_px) * ui_scale;
    let stroke = |x0: f32| {
        let (x1, y0, y1) = (x0 + width, top - extra, bottom + extra);
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    };
    [stroke(right - width), stroke(right + gap)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::construction_view::bar_rects;
    use sf_data::build_stages::BarLook;

    const BAR: BarLook = BarLook {
        fill: (0, 0, 0, 0),
        back: (0, 0, 0, 0),
        height_px: 6,
        min_width_px: 18,
        max_width_px: 220,
        gap_px: 4,
    };

    #[test]
    fn the_readout_says_the_label_and_the_credits() {
        let look = MoneyLook::embedded();
        assert_eq!(readout_text(&look, 1000), "Credits 1000");
        assert_eq!(readout_text(&look, 0), "Credits 0");
    }

    #[test]
    fn few_credits_or_a_waiting_build_use_the_warning_colour() {
        let look = MoneyLook::embedded();
        let c = |c: (u8, u8, u8, u8)| Color(c.0, c.1, c.2, c.3);
        assert_eq!(readout_color(&look, 100, false), c(look.text));
        assert_eq!(readout_color(&look, 99, false), c(look.low));
        assert_eq!(readout_color(&look, 5000, true), c(look.low));
    }

    #[test]
    fn the_strokes_stand_at_the_right_end_of_the_bar_and_out_of_it() {
        let look = MoneyLook::embedded();
        let rects = bar_rects([100.0, 300.0], [200.0, 300.0], 400, &BAR, 1.0).unwrap();
        let [a, b] = pause_strokes(&rects, &look.paused, 1.0);
        // Bar: x 100..200, y 304..310. Strokes: 3 wide, 3 apart, 3 above and below.
        assert_eq!(
            a,
            [
                (197.0, 301.0),
                (200.0, 301.0),
                (200.0, 313.0),
                (197.0, 313.0)
            ]
        );
        assert_eq!(
            b,
            [
                (203.0, 301.0),
                (206.0, 301.0),
                (206.0, 313.0),
                (203.0, 313.0)
            ]
        );
        // The UI scale scales the strokes.
        let rects = bar_rects([100.0, 300.0], [300.0, 300.0], 400, &BAR, 2.0).unwrap();
        let [a, _] = pause_strokes(&rects, &look.paused, 2.0);
        assert_eq!(a[1].0 - a[0].0, 6.0);
    }
}
