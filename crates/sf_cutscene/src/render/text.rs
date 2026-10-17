//! Subtitle text: wrapping and the geometry of the subtitle box.
//!
//! The font, the glyph rasteriser and the atlas are the UI's (`sf_ui::text`,
//! `sf_ui::atlas`), used through their public API. Glyphs are rasterised once
//! per size into a private [`Atlas`]; the renderer uploads it as a texture.

use sf_ui::atlas::Atlas;
use sf_ui::text::{Font, GlyphCache};

use super::plan::Rect;

/// Font, glyph cache and atlas for everything the player writes.
pub struct TextEngine {
    pub font: Font,
    pub cache: GlyphCache,
    pub atlas: Atlas,
}

impl Default for TextEngine {
    fn default() -> TextEngine {
        TextEngine {
            font: Font::embedded(),
            cache: GlyphCache::default(),
            atlas: Atlas::default(),
        }
    }
}

impl TextEngine {
    pub fn width(&self, text: &str, px: f32) -> f32 {
        self.font.measure(text, px).0
    }

    /// Break `text` into lines no wider than `max_w` pixels at size `px`:
    /// greedy at spaces, explicit `\n` kept, a word wider than a whole line is
    /// cut between letters. Never returns an empty list (an empty text is one
    /// empty line); no line is empty unless the text had a blank line.
    pub fn wrap(&self, text: &str, px: f32, max_w: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut current = String::new();
            for word in paragraph.split_whitespace() {
                let candidate = if current.is_empty() {
                    word.to_string()
                } else {
                    format!("{current} {word}")
                };
                if self.width(&candidate, px) <= max_w {
                    current = candidate;
                    continue;
                }
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                // The word alone may still be too wide: cut it.
                let mut rest = word;
                while self.width(rest, px) > max_w {
                    let mut cut = 0;
                    for (i, c) in rest.char_indices() {
                        let end = i + c.len_utf8();
                        if self.width(&rest[..end], px) > max_w {
                            break;
                        }
                        cut = end;
                    }
                    if cut == 0 {
                        // Not even one letter fits: take it anyway.
                        cut = rest.chars().next().map_or(rest.len(), char::len_utf8);
                    }
                    lines.push(rest[..cut].to_string());
                    rest = &rest[cut..];
                }
                current = rest.to_string();
            }
            lines.push(current);
        }
        lines
    }
}

/// Font size in pixels for a picture `height` pixels tall: 3.4 % of the
/// height, so a 1080-line picture gets 37 px, a 720-line one 24 px; never
/// smaller than 14 px (the smallest size that stays readable) or larger than
/// 72 px.
pub fn subtitle_font_px(picture_height: f32) -> f32 {
    (picture_height * 0.034).round().clamp(14.0, 72.0)
}

/// Where the subtitle goes: a box near the bottom of the picture holding the
/// speaker's name and the wrapped line.
#[derive(Clone, Debug, PartialEq)]
pub struct SubtitleLayout {
    pub font_px: f32,
    /// The background box, in target pixels.
    pub rect: Rect,
    pub corner_radius: f32,
    /// Top-left of the speaker line.
    pub name_origin: (f32, f32),
    pub name: String,
    /// The wrapped text with the top-left of each line.
    pub lines: Vec<(String, (f32, f32))>,
    /// Width available to a line of text.
    pub max_text_w: f32,
}

/// Lay out a subtitle inside the picture `vp`.
///
/// The box is as wide as its longest line (at most 70 % of the picture),
/// centred, with its bottom 5 % of the picture height above the picture's
/// bottom edge. If the text would fill more than 40 % of the picture's height
/// the font shrinks (down to 12 px) until it fits.
pub fn layout_subtitle(text: &TextEngine, name: &str, body: &str, vp: &Rect) -> SubtitleLayout {
    let mut px = subtitle_font_px(vp.h);
    loop {
        let layout = layout_at(text, name, body, vp, px);
        if layout.rect.h <= vp.h * 0.4 || px <= 12.0 {
            return layout;
        }
        px -= 1.0;
    }
}

fn layout_at(text: &TextEngine, name: &str, body: &str, vp: &Rect, px: f32) -> SubtitleLayout {
    let pad_x = (px * 0.7).round();
    let pad_y = (px * 0.5).round();
    let max_text_w = (vp.w * 0.7 - 2.0 * pad_x).max(px * 4.0);
    let line_h = text.font.metrics(px).line_height().ceil();
    let lines = text.wrap(body, px, max_text_w);
    let widest = lines
        .iter()
        .map(|l| text.width(l, px))
        .chain(std::iter::once(text.width(name, px)))
        .fold(0.0_f32, f32::max);
    let w = (widest.ceil() + 2.0 * pad_x).min(vp.w);
    let h = pad_y * 2.0 + line_h * (1 + lines.len()) as f32;
    let x = (vp.x + (vp.w - w) / 2.0).round();
    let y = (vp.y + vp.h - vp.h * 0.05 - h).round();
    let text_x = x + pad_x;
    let first = y + pad_y;
    SubtitleLayout {
        font_px: px,
        rect: Rect { x, y, w, h },
        corner_radius: (px * 0.5).round(),
        name_origin: (text_x, first),
        name: name.to_string(),
        lines: lines
            .into_iter()
            .enumerate()
            .map(|(i, l)| (l, (text_x, first + line_h * (i + 1) as f32)))
            .collect(),
        max_text_w,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp(w: f32, h: f32) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        }
    }

    #[test]
    fn font_size_follows_the_picture_height() {
        assert_eq!(subtitle_font_px(1080.0), 37.0);
        assert_eq!(subtitle_font_px(720.0), 24.0);
        assert_eq!(subtitle_font_px(100.0), 14.0, "never below 14 px");
        assert_eq!(subtitle_font_px(10_000.0), 72.0);
    }

    #[test]
    fn wrap_keeps_every_word_and_respects_the_width() {
        let t = TextEngine::default();
        let text = "Two bridges. We can't hold both. The engineers can drop the west one, \
                    but they need twelve minutes and nobody shooting at them.";
        for width in [200.0, 330.0, 700.0] {
            let lines = t.wrap(text, 24.0, width);
            assert!(lines.len() > 1);
            for l in &lines {
                assert!(!l.is_empty());
                assert!(t.width(l, 24.0) <= width, "{l:?} wider than {width}");
            }
            let joined: Vec<&str> = lines.iter().flat_map(|l| l.split(' ')).collect();
            let want: Vec<&str> = text.split_whitespace().collect();
            assert_eq!(joined, want, "words kept, in order");
        }
        assert_eq!(t.wrap("short", 24.0, 500.0), ["short"]);
        assert_eq!(t.wrap("", 24.0, 500.0), [""]);
    }

    #[test]
    fn wrap_keeps_explicit_breaks_and_cuts_a_giant_word() {
        let t = TextEngine::default();
        assert_eq!(t.wrap("one\ntwo", 24.0, 500.0), ["one", "two"]);
        let lines = t.wrap("Supercalifragilisticexpialidocious", 24.0, 120.0);
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|l| t.width(l, 24.0) <= 120.0));
        assert_eq!(lines.concat(), "Supercalifragilisticexpialidocious");
        // A width smaller than one letter still terminates.
        let lines = t.wrap("abc", 24.0, 1.0);
        assert_eq!(lines.concat(), "abc");
    }

    #[test]
    fn the_subtitle_box_sits_inside_the_picture_at_both_resolutions() {
        let t = TextEngine::default();
        let body = "Then you give the engineers their twelve minutes. Keep the post standing \
                    until that bridge goes into the water. After that, nothing crosses in the east.";
        for (w, h) in [(1920.0, 1080.0), (1280.0, 720.0), (640.0, 360.0)] {
            let v = vp(w, h);
            let l = layout_subtitle(&t, "Major Halversen", body, &v);
            assert!(l.rect.x >= 0.0 && l.rect.right() <= w, "{l:?}");
            assert!(l.rect.y >= h * 0.5, "the box is in the lower half: {l:?}");
            assert!(l.rect.bottom() <= h * 0.96, "and above the bottom edge");
            assert!(
                l.lines.len() >= 2 && l.lines.len() <= 4,
                "{} lines",
                l.lines.len()
            );
            for (text, (x, y)) in &l.lines {
                assert!(*x >= l.rect.x && *y + l.font_px <= l.rect.bottom() + 1.0);
                assert!(t.width(text, l.font_px) <= l.max_text_w);
            }
            // Readable: 720p gets at least 22 px, 1080p at least 34 px.
            if h >= 1080.0 {
                assert!(l.font_px >= 34.0);
            } else if h >= 720.0 {
                assert!(l.font_px >= 22.0);
            }
        }
    }

    #[test]
    fn a_huge_line_shrinks_the_font_to_fit() {
        let t = TextEngine::default();
        let body = "word ".repeat(400);
        let v = vp(1280.0, 720.0);
        let l = layout_subtitle(&t, "A", &body, &v);
        assert!(l.font_px < subtitle_font_px(720.0));
        assert!(l.rect.h <= v.h * 0.4 + 1.0 || l.font_px <= 12.0);
    }
}
