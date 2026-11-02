//! How the end of a match is shown (WP7.6), in `assets/config/match_look.ron`: the banner with the
//! word for the result. Presentation only; the result itself is the simulation's
//! (`docs/rules/match.md`).
//!
//! ```
//! use sf_data::match_look::MatchLook;
//!
//! let look = MatchLook::embedded();
//! assert_eq!(look.victory.title, "Victory");
//! ```

use serde::Deserialize;

/// The shipped file, compiled in, like the other tables.
pub const EMBEDDED_MATCH_LOOK: &str = include_str!("../../../assets/config/match_look.ron");

/// The banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerLook {
    /// Width in logical pixels.
    pub width_px: u16,
    /// Height in logical pixels.
    pub height_px: u16,
    /// Distance from the top edge of the window.
    pub top_px: u16,
    /// Size of the big word.
    pub title_px: u16,
    /// Size of the line under it.
    pub detail_px: u16,
    /// Gap between the two.
    pub gap_px: u16,
    /// Width of the frame.
    pub border_px: u16,
}

/// The colours and the word of one result.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultLook {
    /// The big word.
    pub title: String,
    /// The colour of the text.
    pub text: (u8, u8, u8, u8),
    /// The colour of the banner.
    pub back: (u8, u8, u8, u8),
    /// The colour of the frame.
    pub border: (u8, u8, u8, u8),
}

/// The look of the end of a match.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchLook {
    /// The banner.
    pub banner: BannerLook,
    /// The local player won.
    pub victory: ResultLook,
    /// The local player is defeated.
    pub defeat: ResultLook,
    /// Nobody won.
    pub draw: ResultLook,
    /// The line under the word; `{time}` and `{tick}` are replaced.
    pub detail: String,
}

impl MatchLook {
    /// Reads and checks the look. The error says what is wrong.
    pub fn parse(text: &str) -> Result<MatchLook, String> {
        let look: MatchLook = ron::from_str(text).map_err(|e| e.to_string())?;
        let b = &look.banner;
        if b.width_px == 0 || b.height_px == 0 || b.title_px == 0 || b.detail_px == 0 {
            return Err(
                "banner.width_px, height_px, title_px and detail_px must be above 0".into(),
            );
        }
        if u32::from(b.title_px) + u32::from(b.detail_px) + u32::from(b.gap_px)
            > u32::from(b.height_px)
        {
            return Err("banner.height_px is too small for the word and the line under it".into());
        }
        for (name, r) in [
            ("victory", &look.victory),
            ("defeat", &look.defeat),
            ("draw", &look.draw),
        ] {
            if r.title.trim().is_empty() {
                return Err(format!("{name}.title must not be empty"));
            }
        }
        if !look.detail.contains("{time}") && !look.detail.contains("{tick}") {
            return Err("detail must say when: it needs {time} or {tick}".into());
        }
        Ok(look)
    }

    /// The shipped look. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> MatchLook {
        MatchLook::parse(EMBEDDED_MATCH_LOOK)
            .unwrap_or_else(|e| panic!("assets/config/match_look.ron: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_look_is_pinned() {
        let look = MatchLook::embedded();
        assert_eq!(
            look.banner,
            BannerLook {
                width_px: 460,
                height_px: 118,
                top_px: 96,
                title_px: 52,
                detail_px: 18,
                gap_px: 6,
                border_px: 3,
            }
        );
        assert_eq!(look.victory.title, "Victory");
        assert_eq!(look.victory.text, (150, 235, 140, 255));
        assert_eq!(look.victory.back, (14, 44, 20, 232));
        assert_eq!(look.victory.border, (110, 200, 100, 255));
        assert_eq!(look.defeat.title, "Defeat");
        assert_eq!(look.defeat.text, (255, 130, 110, 255));
        assert_eq!(look.defeat.back, (52, 16, 14, 232));
        assert_eq!(look.defeat.border, (220, 90, 70, 255));
        assert_eq!(look.draw.title, "Draw");
        assert_eq!(look.draw.text, (235, 215, 140, 255));
        assert_eq!(look.draw.back, (44, 38, 14, 232));
        assert_eq!(look.draw.border, (210, 190, 100, 255));
        assert_eq!(look.detail, "decided after {time} (tick {tick})");
    }

    #[test]
    fn a_wrong_look_is_refused_with_the_reason() {
        let ok = EMBEDDED_MATCH_LOOK;
        let bad = |from: &str, to: &str, what: &str| {
            let e = MatchLook::parse(&ok.replace(from, to)).unwrap_err();
            assert!(e.contains(what), "{e:?} should mention {what:?}");
        };
        bad("width_px: 460", "width_px: 0", "above 0");
        bad("height_px: 118", "height_px: 60", "too small");
        bad("title: \"Draw\"", "title: \" \"", "draw.title");
        bad("title: \"Victory\"", "title: \"\"", "victory.title");
        bad(
            "detail: \"decided after {time} (tick {tick})\"",
            "detail: \"decided\"",
            "when",
        );
        bad("border_px: 3,", "border_px: 3, extra: 1,", "extra");
    }
}
