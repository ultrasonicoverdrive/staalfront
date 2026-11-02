//! The end of a match in the player HUD (WP7.6, `docs/rules/match.md`). Presentation only: who is
//! defeated and who won is the simulation's (`sf_sim::outcome`), copied into every snapshot
//! ([`SimSnapshot::match_result`]); nothing here decides anything.
//!
//! When the result is decided a **banner** stands at the top middle of the window: the word for the
//! local player's result ("Victory", "Defeat" or "Draw") and, under it, when the match was decided
//! (game time and tick). The game keeps running behind it: the result never changes, and the player
//! may carry on (there is no pause and no menu before the sidebar and menus of phase 11). Every number,
//! colour and word is `assets/config/match_look.ron` ([`sf_data::match_look`]).

use sf_data::match_look::{MatchLook, ResultLook};
use sf_math::TICKS_PER_SECOND;
use sf_sim::{MatchResult, PlayerId, Verdict};
use sf_ui::{Anchor, Color, Ui};

/// The shipped look, read once.
pub fn shipped_look() -> &'static MatchLook {
    static LOOK: std::sync::OnceLock<MatchLook> = std::sync::OnceLock::new();
    LOOK.get_or_init(MatchLook::embedded)
}

/// How the match ended for the local player.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    /// The local player won.
    Victory,
    /// Somebody else won.
    Defeat,
    /// Nobody won.
    Draw,
}

/// What the HUD shows about the end of the match.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    /// The result for the local player.
    pub standing: Standing,
    /// The tick the match was decided in.
    pub tick: u64,
}

/// The result of the match for `local`: a win of its own is a victory, a win of anyone else a defeat.
pub fn standing_of(result: MatchResult, local: PlayerId) -> Standing {
    match result.verdict {
        Verdict::Won(winner) if winner == local => Standing::Victory,
        Verdict::Won(_) => Standing::Defeat,
        Verdict::Draw => Standing::Draw,
    }
}

/// The banner for the decided result, if there is one.
pub fn banner_of(result: Option<MatchResult>, local: PlayerId) -> Option<Banner> {
    result.map(|r| Banner {
        standing: standing_of(r, local),
        tick: r.tick.0,
    })
}

/// Game time as `minutes:seconds` (seconds with two digits) for a tick.
pub fn clock(tick: u64) -> String {
    let seconds = tick / u64::from(TICKS_PER_SECOND);
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The look of one standing.
pub fn result_look(look: &MatchLook, standing: Standing) -> &ResultLook {
    match standing {
        Standing::Victory => &look.victory,
        Standing::Defeat => &look.defeat,
        Standing::Draw => &look.draw,
    }
}

/// The word and the line under it.
pub fn texts(look: &MatchLook, banner: &Banner) -> (String, String) {
    let detail = look
        .detail
        .replace("{time}", &clock(banner.tick))
        .replace("{tick}", &banner.tick.to_string());
    (result_look(look, banner.standing).title.clone(), detail)
}

fn color(c: (u8, u8, u8, u8)) -> Color {
    Color(c.0, c.1, c.2, c.3)
}

/// Draws the banner on `ui`. Call it while the UI of the frame is being built, after the other panels.
pub fn draw_banner(ui: &mut Ui, banner: &Banner, look: &MatchLook) {
    let b = &look.banner;
    let scale = ui.scale();
    let size = (f32::from(b.width_px), f32::from(b.height_px));
    let rect = ui.place(ui.screen(), Anchor::Top, (0.0, f32::from(b.top_px)), size);
    let r = result_look(look, banner.standing);
    let border = (f32::from(b.border_px) * scale).round();
    ui.fill(rect, color(r.border));
    let inner = sf_ui::Rect::new(
        rect.x + border,
        rect.y + border,
        (rect.w - 2.0 * border).max(0.0),
        (rect.h - 2.0 * border).max(0.0),
    );
    ui.fill(inner, color(r.back));
    let (title, detail) = texts(look, banner);
    let (title_px, detail_px) = (
        f32::from(b.title_px) * scale,
        f32::from(b.detail_px) * scale,
    );
    let gap = f32::from(b.gap_px) * scale;
    let (tw, th) = ui.measure_px(&title, title_px);
    let (dw, dh) = ui.measure_px(&detail, detail_px);
    // The two lines as one block, centred in the banner.
    let top = rect.y + (rect.h - (th + gap + dh)) * 0.5;
    ui.text_px_at(
        rect.x + (rect.w - tw) * 0.5,
        top,
        &title,
        title_px,
        color(r.text),
    );
    ui.text_px_at(
        rect.x + (rect.w - dw) * 0.5,
        top + th + gap,
        &detail,
        detail_px,
        color(r.text),
    );
}

/// What the HUD draws for the state of the match: the banner for the result as the local player
/// sees it, once it is decided, and nothing before. The one function the HUD calls
/// (`hud::Hud::frame`) and the screenshot test calls. Returns whether a banner was drawn.
pub fn draw_result(ui: &mut Ui, result: Option<MatchResult>, local: PlayerId) -> bool {
    match banner_of(result, local) {
        Some(banner) => {
            draw_banner(ui, &banner, shipped_look());
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_sim::Tick;

    fn won(by: u8, tick: u64) -> MatchResult {
        MatchResult {
            tick: Tick(tick),
            verdict: Verdict::Won(PlayerId(by)),
        }
    }

    #[test]
    fn the_standing_is_the_local_players() {
        // Distinct players: the winner is player 3, the local one 0, then 3.
        assert_eq!(standing_of(won(3, 10), PlayerId(0)), Standing::Defeat);
        assert_eq!(standing_of(won(3, 10), PlayerId(3)), Standing::Victory);
        let draw = MatchResult {
            tick: Tick(10),
            verdict: Verdict::Draw,
        };
        assert_eq!(standing_of(draw, PlayerId(0)), Standing::Draw);
    }

    #[test]
    fn there_is_no_banner_before_the_result() {
        assert_eq!(banner_of(None, PlayerId(0)), None);
        assert_eq!(
            banner_of(Some(won(0, 5687)), PlayerId(0)),
            Some(Banner {
                standing: Standing::Victory,
                tick: 5687
            })
        );
    }

    #[test]
    fn the_clock_counts_game_seconds_at_twenty_ticks_to_the_second() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(19), "0:00");
        assert_eq!(clock(20), "0:01");
        assert_eq!(clock(1199), "0:59");
        assert_eq!(clock(1200), "1:00");
        // The match of the headless scenario: tick 5687 is 284 s.
        assert_eq!(clock(5687), "4:44");
    }

    #[test]
    fn the_words_come_from_the_look() {
        let look = shipped_look();
        let banner = |standing| Banner {
            standing,
            tick: 5687,
        };
        assert_eq!(
            texts(look, &banner(Standing::Victory)),
            (
                "Victory".to_string(),
                "decided after 4:44 (tick 5687)".to_string()
            )
        );
        assert_eq!(texts(look, &banner(Standing::Defeat)).0, "Defeat");
        assert_eq!(texts(look, &banner(Standing::Draw)).0, "Draw");
        // And a changed look changes the words: nothing is written here.
        let mut other = look.clone();
        other.victory.title = "Won".into();
        other.detail = "at {tick}".into();
        assert_eq!(
            texts(&other, &banner(Standing::Victory)),
            ("Won".to_string(), "at 5687".to_string())
        );
    }
}
