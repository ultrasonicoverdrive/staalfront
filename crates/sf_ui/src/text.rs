//! Text: font metrics, layout, and the glyph cache backed by the atlas.
//!
//! The font is a 52 KB subset of Inter Medium (SIL OFL, see
//! `assets/LICENSES.md`) embedded in the binary. [`ab_glyph`] gives outlines,
//! advances and (legacy `kern` table) kerning; glyphs are rasterized once per
//! `(char, pixel size)` into the shared [`Atlas`] as white RGBA with the
//! coverage in alpha, so text is tinted by the vertex color like everything
//! else. Layout (measuring, line breaks) is pure CPU and needs no atlas.
//!
//! The text is UTF-8 and the font subset covers Basic Latin, Latin-1
//! Supplement and Latin Extended-A, so Dutch diacritics (e, e, i with
//! diaeresis or acute) work. A character the font lacks draws the font's
//! "missing glyph" box.

use std::collections::HashMap;

use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont as _};

use crate::atlas::{Atlas, AtlasRect};

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium-subset.otf");

/// Smallest and largest pixel size that is rasterized.
const MIN_PX: f32 = 6.0;
const MAX_PX: f32 = 160.0;

/// Where one character sits: pen position of its origin on the baseline,
/// relative to the top-left of the text block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedChar {
    pub ch: char,
    /// Pen x of the glyph origin.
    pub x: f32,
    /// Baseline y.
    pub baseline: f32,
}

/// Vertical metrics of the font at a pixel size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    pub ascent: f32,
    /// Positive distance below the baseline.
    pub descent: f32,
    pub line_gap: f32,
}

impl LineMetrics {
    /// Distance from one baseline to the next.
    pub fn line_height(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }
}

/// The embedded font.
pub struct Font {
    font: FontRef<'static>,
}

impl Default for Font {
    fn default() -> Self {
        Self::embedded()
    }
}

impl Font {
    pub fn embedded() -> Self {
        let font = FontRef::try_from_slice(FONT_BYTES).expect("the embedded font is valid");
        Self { font }
    }

    /// `px` is the em size in pixels (the usual meaning of a font size).
    fn scale(&self, px: f32) -> PxScale {
        let px = px.clamp(MIN_PX, MAX_PX);
        let upm = self.font.units_per_em().unwrap_or(1000.0);
        PxScale::from(px * self.font.height_unscaled() / upm)
    }

    pub fn metrics(&self, px: f32) -> LineMetrics {
        let s = self.font.as_scaled(self.scale(px));
        LineMetrics {
            ascent: s.ascent(),
            descent: -s.descent(),
            line_gap: s.line_gap(),
        }
    }

    /// Advance of `ch` at `px`.
    pub fn advance(&self, ch: char, px: f32) -> f32 {
        let s = self.font.as_scaled(self.scale(px));
        s.h_advance(s.glyph_id(ch))
    }

    /// Place every character of `text` (lines split at `\n`, kerning applied).
    /// Returns the glyphs and the size of the block.
    pub fn layout(&self, text: &str, px: f32) -> (Vec<PlacedChar>, (f32, f32)) {
        let s = self.font.as_scaled(self.scale(px));
        let m = self.metrics(px);
        let mut out = Vec::with_capacity(text.len());
        let (mut width, mut lines) = (0.0f32, 0u32);
        for (i, line) in text.split('\n').enumerate() {
            let baseline = m.ascent + i as f32 * m.line_height();
            let mut x = 0.0f32;
            let mut prev = None;
            for ch in line.chars() {
                let id = s.glyph_id(ch);
                if let Some(p) = prev {
                    x += s.kern(p, id);
                }
                out.push(PlacedChar { ch, x, baseline });
                x += s.h_advance(id);
                prev = Some(id);
            }
            width = width.max(x);
            lines += 1;
        }
        let height = m.ascent + m.descent + (lines.saturating_sub(1)) as f32 * m.line_height();
        (out, (width, height))
    }

    /// Size of `text` as laid out: widest line, and ascent + descent plus
    /// the extra lines.
    pub fn measure(&self, text: &str, px: f32) -> (f32, f32) {
        self.layout(text, px).1
    }

    /// Coverage bitmap of `ch` at `px`, or `None` for glyphs without an
    /// outline (space). `left`/`top` are the offsets of the bitmap's top-left
    /// from the pen origin on the baseline (`top` is negative above it).
    fn rasterize(&self, ch: char, px: f32) -> Option<Raster> {
        let scale = self.scale(px);
        let glyph = self
            .font
            .glyph_id(ch)
            .with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
        let outlined = self.font.outline_glyph(glyph)?;
        let b = outlined.px_bounds();
        let (w, h) = (
            (b.max.x - b.min.x).ceil() as u32,
            (b.max.y - b.min.y).ceil() as u32,
        );
        if w == 0 || h == 0 {
            return None;
        }
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        outlined.draw(|x, y, c| {
            if x < w && y < h {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 3].fill(255);
                rgba[i + 3] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        Some(Raster {
            w,
            h,
            left: b.min.x.floor(),
            top: b.min.y.floor(),
            rgba,
        })
    }
}

struct Raster {
    w: u32,
    h: u32,
    left: f32,
    top: f32,
    rgba: Vec<u8>,
}

/// A glyph in the atlas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphSlot {
    pub rect: AtlasRect,
    pub left: f32,
    pub top: f32,
}

/// Rasterized glyphs by `(char, pixel size)`. `None` = no outline (space) or
/// the atlas was full.
#[derive(Default)]
pub struct GlyphCache {
    slots: HashMap<(char, u16), Option<GlyphSlot>>,
    /// Glyphs that could not be packed (atlas full), for diagnostics.
    pub dropped: u32,
}

impl GlyphCache {
    /// The cached glyph, rasterizing and packing it on first use. `px` is
    /// rounded to whole pixels (the cache key).
    pub fn get(&mut self, font: &Font, atlas: &mut Atlas, ch: char, px: f32) -> Option<GlyphSlot> {
        let px = px.clamp(MIN_PX, MAX_PX).round();
        *self.slots.entry((ch, px as u16)).or_insert_with(|| {
            let raster = font.rasterize(ch, px)?;
            let rect = atlas.add(raster.w, raster.h, &raster.rgba);
            if rect.is_none() {
                self.dropped += 1;
            }
            Some(GlyphSlot {
                rect: rect?,
                left: raster.left,
                top: raster.top,
            })
        })
    }

    /// Number of distinct glyph sizes cached (spaces included).
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_scales_with_size_and_grows_with_text() {
        let f = Font::embedded();
        let (w14, h14) = f.measure("Hello", 14.0);
        let (w28, h28) = f.measure("Hello", 28.0);
        assert!(w14 > 20.0 && w14 < 60.0, "plausible width {w14}");
        assert!((w28 / w14 - 2.0).abs() < 0.05, "{w28} vs {w14}");
        assert!((h28 / h14 - 2.0).abs() < 0.05);
        assert!(f.measure("Hello world", 14.0).0 > w14);
        assert_eq!(f.measure("", 14.0).0, 0.0);
    }

    #[test]
    fn height_is_font_metrics_not_glyph_shapes() {
        let f = Font::embedded();
        assert_eq!(f.measure("a", 20.0).1, f.measure("Qg", 20.0).1);
        let one = f.measure("a", 20.0).1;
        let two = f.measure("a\nb", 20.0).1;
        assert!((two - one - f.metrics(20.0).line_height()).abs() < 1e-3);
    }

    #[test]
    fn width_is_the_widest_line_and_matches_the_sum_of_advances() {
        let f = Font::embedded();
        let (glyphs, (w, _)) = f.layout("ab\nabcd", 16.0);
        let line2: Vec<_> = glyphs
            .iter()
            .filter(|g| g.baseline > glyphs[0].baseline)
            .collect();
        assert_eq!(line2.len(), 4);
        assert_eq!(line2[0].x, 0.0);
        let sum: f32 = "abcd".chars().map(|c| f.advance(c, 16.0)).sum();
        assert!((w - sum).abs() < 2.0, "{w} vs {sum} (kerning may differ)");
        assert_eq!(w, f.measure("abcd", 16.0).0);
    }

    #[test]
    fn dutch_diacritics_are_in_the_font() {
        let f = Font::embedded();
        let mut atlas = Atlas::default();
        let mut cache = GlyphCache::default();
        let notdef = f.font.glyph_id('\u{10FFFF}');
        for ch in "éëïèüöÉËÏ€’".chars() {
            assert_ne!(
                f.font.glyph_id(ch),
                notdef,
                "{ch} maps to the missing glyph"
            );
            assert!(
                cache.get(&f, &mut atlas, ch, 16.0).is_some(),
                "{ch} has an outline"
            );
        }
    }

    #[test]
    fn rasterized_glyph_has_coverage_and_sits_on_the_baseline() {
        let f = Font::embedded();
        let mut atlas = Atlas::default();
        let mut cache = GlyphCache::default();
        let slot = cache.get(&f, &mut atlas, 'H', 20.0).expect("H");
        assert!(slot.top < 0.0, "capital H rises above the baseline");
        assert!(
            slot.top + slot.rect.h as f32 >= -1.0,
            "and ends at the baseline"
        );
        let mut max_alpha = 0;
        for y in 0..slot.rect.h {
            for x in 0..slot.rect.w {
                max_alpha = max_alpha.max(atlas.texel(slot.rect.x + x, slot.rect.y + y)[3]);
            }
        }
        assert_eq!(max_alpha, 255, "a stem pixel is fully covered");
        // Second lookup is a cache hit: nothing new is packed.
        let before = atlas.pixels().to_vec();
        assert_eq!(cache.get(&f, &mut atlas, 'H', 20.0), Some(slot));
        assert_eq!(atlas.pixels(), &before[..]);
    }

    #[test]
    fn space_has_no_bitmap_but_an_advance() {
        let f = Font::embedded();
        let mut atlas = Atlas::default();
        let mut cache = GlyphCache::default();
        assert!(cache.get(&f, &mut atlas, ' ', 16.0).is_none());
        assert!(f.advance(' ', 16.0) > 2.0);
    }

    #[test]
    fn sizes_are_cached_separately_and_a_full_atlas_degrades_quietly() {
        let f = Font::embedded();
        let mut atlas = Atlas::with_limits(16, 16);
        let mut cache = GlyphCache::default();
        let mut got = 0;
        for ch in "abcdefghijklmnopqrstuvwxyz".chars() {
            got += usize::from(cache.get(&f, &mut atlas, ch, 24.0).is_some());
        }
        assert!(got < 26, "a 512x16 atlas cannot hold 26 big glyphs");
        assert!(cache.dropped > 0);
        let mut atlas = Atlas::default();
        let mut cache = GlyphCache::default();
        cache.get(&f, &mut atlas, 'a', 12.0);
        cache.get(&f, &mut atlas, 'a', 13.0);
        assert_eq!(cache.len(), 2);
    }
}
