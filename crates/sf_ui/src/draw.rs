//! The draw list: everything the UI draws in a frame as one vertex/index
//! buffer plus a few batches.
//!
//! Every primitive is a textured quad; a solid rectangle samples the white
//! texel of the atlas. Quads are appended in painter's order and consecutive
//! quads with the same texture merge into one batch, so a frame is normally
//! two draw calls: the atlas (panels, text, buttons, lines) and, if a minimap
//! is shown, the minimap texture.
//!
//! UV coordinates are in texels; the shader divides by the texture size, so
//! quads recorded before the atlas grew stay valid.

use bytemuck::{Pod, Zeroable};

use crate::color::Color;
use crate::layout::Rect;
use crate::skin::NineSlice;

/// Which texture a batch samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Texture {
    /// Glyphs and skin.
    Atlas,
    /// The same atlas, read as **colour pictures** (WP3.5, the cursors): their
    /// texels are sRGB colours, decoded to linear light by the GPU when the
    /// target is an sRGB format. The glyphs and the skin are coverage and
    /// shading, tinted by the vertex colour, and must not be decoded.
    Sprites,
    /// The baked terrain image of the minimap.
    Minimap,
}

/// One UI vertex: position in window pixels, UV in texels, sRGB color.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

/// A run of indices drawn with one texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    pub texture: Texture,
    pub start: u32,
    pub end: u32,
}

/// UV rectangle in texels: `[u0, v0, u1, v1]`.
pub type Uv = [f32; 4];

#[derive(Clone, Debug, Default)]
pub struct DrawList {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<Batch>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.batches.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Number of quads recorded.
    pub fn quad_count(&self) -> usize {
        self.indices.len() / 6
    }

    fn batch(&mut self, texture: Texture) {
        let end = self.indices.len() as u32;
        match self.batches.last_mut() {
            Some(b) if b.texture == texture => b.end = end,
            _ => self.batches.push(Batch {
                texture,
                start: end - 6,
                end,
            }),
        }
    }

    /// An axis-aligned quad.
    pub fn quad(&mut self, texture: Texture, rect: Rect, uv: Uv, color: Color) {
        if rect.w <= 0.0 || rect.h <= 0.0 || color.3 == 0 {
            return;
        }
        self.quad_points(
            texture,
            [
                (rect.x, rect.y),
                (rect.right(), rect.y),
                (rect.right(), rect.bottom()),
                (rect.x, rect.bottom()),
            ],
            uv,
            color,
        );
    }

    /// A quad with arbitrary corners (clockwise from top-left), same UVs.
    pub fn quad_points(&mut self, texture: Texture, p: [(f32, f32); 4], uv: Uv, color: Color) {
        let base = self.vertices.len() as u32;
        let c = color.to_array();
        let corners = [
            (p[0], [uv[0], uv[1]]),
            (p[1], [uv[2], uv[1]]),
            (p[2], [uv[2], uv[3]]),
            (p[3], [uv[0], uv[3]]),
        ];
        for ((x, y), uv) in corners {
            self.vertices.push(Vertex {
                pos: [x, y],
                uv,
                color: c,
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        self.batch(texture);
    }

    /// A solid rectangle (samples the white texel at `white`).
    pub fn rect(&mut self, white: (f32, f32), rect: Rect, color: Color) {
        let uv = [white.0, white.1, white.0, white.1];
        self.quad(Texture::Atlas, rect, uv, color);
    }

    /// A rectangle outline of `thickness` pixels, drawn inside `rect`.
    pub fn outline(&mut self, white: (f32, f32), rect: Rect, thickness: f32, color: Color) {
        let t = thickness.min(rect.w * 0.5).min(rect.h * 0.5);
        self.rect(white, Rect::new(rect.x, rect.y, rect.w, t), color);
        self.rect(
            white,
            Rect::new(rect.x, rect.bottom() - t, rect.w, t),
            color,
        );
        self.rect(
            white,
            Rect::new(rect.x, rect.y + t, t, rect.h - 2.0 * t),
            color,
        );
        self.rect(
            white,
            Rect::new(rect.right() - t, rect.y + t, t, rect.h - 2.0 * t),
            color,
        );
    }

    /// A line segment of `thickness` pixels (a rotated quad).
    pub fn line(
        &mut self,
        white: (f32, f32),
        a: (f32, f32),
        b: (f32, f32),
        thickness: f32,
        color: Color,
    ) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-3 || color.3 == 0 {
            return;
        }
        let (nx, ny) = (-dy / len * thickness * 0.5, dx / len * thickness * 0.5);
        let uv = [white.0, white.1, white.0, white.1];
        self.quad_points(
            Texture::Atlas,
            [
                (a.0 + nx, a.1 + ny),
                (b.0 + nx, b.1 + ny),
                (b.0 - nx, b.1 - ny),
                (a.0 - nx, a.1 - ny),
            ],
            uv,
            color,
        );
    }

    /// A nine-slice image stretched over `dst`. `border` is the size of the
    /// corners on screen in pixels; it shrinks when `dst` is smaller than
    /// two borders. Nine quads, no gaps.
    pub fn nine_slice(&mut self, ns: &NineSlice, dst: Rect, border: f32, color: Color) {
        let b = border.min(dst.w * 0.5).min(dst.h * 0.5).max(0.0);
        let r = ns.rect;
        let tb = ns.border;
        let xs = [dst.x, dst.x + b, dst.right() - b, dst.right()];
        let ys = [dst.y, dst.y + b, dst.bottom() - b, dst.bottom()];
        let us = [
            r.x as f32,
            r.x as f32 + tb,
            (r.x + r.w) as f32 - tb,
            (r.x + r.w) as f32,
        ];
        let vs = [
            r.y as f32,
            r.y as f32 + tb,
            (r.y + r.h) as f32 - tb,
            (r.y + r.h) as f32,
        ];
        for j in 0..3 {
            for i in 0..3 {
                let piece = Rect::new(xs[i], ys[j], xs[i + 1] - xs[i], ys[j + 1] - ys[j]);
                self.quad(
                    Texture::Atlas,
                    piece,
                    [us[i], vs[j], us[i + 1], vs[j + 1]],
                    color,
                );
            }
        }
    }
}

/// A drawing handle on a [`DrawList`] for the game's own overlays: solid
/// rectangles, outlines and lines in window pixels, none of which blocks
/// input. [`Ui::world_layer`](crate::Ui::world_layer) hands out the one that
/// is drawn **under** the panels (selection rings, health bars and the drag
/// rectangle of WP3.3 live there), so a panel never has a ring painted over
/// it.
pub struct Layer<'a> {
    list: &'a mut DrawList,
    white: (f32, f32),
}

impl<'a> Layer<'a> {
    pub(crate) fn new(list: &'a mut DrawList, white: (f32, f32)) -> Self {
        Self { list, white }
    }

    /// A solid rectangle.
    pub fn fill(&mut self, rect: Rect, color: Color) {
        self.list.rect(self.white, rect, color);
    }

    /// A rectangle outline of `thickness` pixels, drawn inside `rect`.
    pub fn outline(&mut self, rect: Rect, thickness: f32, color: Color) {
        self.list.outline(self.white, rect, thickness, color);
    }

    /// A solid quad with arbitrary corners, clockwise from the top-left on the
    /// screen: a cell of the ground seen in perspective (WP6.2).
    pub fn fill_quad(&mut self, corners: [(f32, f32); 4], color: Color) {
        if color.3 == 0 {
            return;
        }
        let uv = [self.white.0, self.white.1, self.white.0, self.white.1];
        self.list.quad_points(Texture::Atlas, corners, uv, color);
    }

    /// A line segment of `thickness` pixels.
    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), thickness: f32, color: Color) {
        self.list.line(self.white, a, b, thickness, color);
    }

    /// Segments through `points`; with `closed` the last point joins the
    /// first.
    pub fn polyline(&mut self, points: &[[f32; 2]], closed: bool, thickness: f32, color: Color) {
        for pair in points.windows(2) {
            self.line(
                (pair[0][0], pair[0][1]),
                (pair[1][0], pair[1][1]),
                thickness,
                color,
            );
        }
        if closed && points.len() > 2 {
            let (a, b) = (points[points.len() - 1], points[0]);
            self.line((a[0], a[1]), (b[0], b[1]), thickness, color);
        }
    }

    /// Quads recorded in the whole list so far.
    pub fn quad_count(&self) -> usize {
        self.list.quad_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::AtlasRect;

    const WHITE: (f32, f32) = (1.0, 1.0);

    #[test]
    fn fill_quad_is_one_solid_quad_with_the_given_corners() {
        let mut d = DrawList::default();
        let corners = [(10.0, 10.0), (50.0, 12.0), (60.0, 40.0), (5.0, 38.0)];
        Layer::new(&mut d, WHITE).fill_quad(corners, Color(1, 2, 3, 200));
        assert_eq!(
            (d.vertices.len(), d.indices.len(), d.quad_count()),
            (4, 6, 1)
        );
        let got: Vec<(f32, f32)> = d.vertices.iter().map(|v| (v.pos[0], v.pos[1])).collect();
        assert_eq!(got, corners.to_vec());
        // Every corner samples the white texel.
        assert!(d.vertices.iter().all(|v| v.uv == [WHITE.0, WHITE.1]));
        // A fully transparent quad is not drawn.
        Layer::new(&mut d, WHITE).fill_quad(corners, Color(1, 2, 3, 0));
        assert_eq!(d.quad_count(), 1);
    }

    #[test]
    fn rect_makes_one_quad_and_merges_batches() {
        let mut d = DrawList::default();
        d.rect(WHITE, Rect::new(0.0, 0.0, 10.0, 10.0), Color::WHITE);
        d.rect(WHITE, Rect::new(20.0, 0.0, 10.0, 10.0), Color::WHITE);
        assert_eq!(
            (d.vertices.len(), d.indices.len(), d.quad_count()),
            (8, 12, 2)
        );
        assert_eq!(
            d.batches,
            vec![Batch {
                texture: Texture::Atlas,
                start: 0,
                end: 12
            }]
        );
    }

    #[test]
    fn texture_change_starts_a_new_batch_and_order_is_kept() {
        let mut d = DrawList::default();
        let r = Rect::new(0.0, 0.0, 4.0, 4.0);
        d.rect(WHITE, r, Color::WHITE);
        d.quad(Texture::Minimap, r, [0.0, 0.0, 8.0, 8.0], Color::WHITE);
        d.rect(WHITE, r, Color::WHITE);
        let kinds: Vec<_> = d
            .batches
            .iter()
            .map(|b| (b.texture, b.start, b.end))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (Texture::Atlas, 0, 6),
                (Texture::Minimap, 6, 12),
                (Texture::Atlas, 12, 18)
            ]
        );
    }

    #[test]
    fn empty_and_transparent_quads_are_skipped() {
        let mut d = DrawList::default();
        d.rect(WHITE, Rect::new(0.0, 0.0, 0.0, 10.0), Color::WHITE);
        d.rect(WHITE, Rect::new(0.0, 0.0, 10.0, 10.0), Color::TRANSPARENT);
        assert!(d.is_empty());
    }

    #[test]
    fn nine_slice_tiles_the_destination_exactly() {
        let ns = NineSlice {
            rect: AtlasRect {
                x: 100,
                y: 50,
                w: 16,
                h: 16,
            },
            border: 6.0,
        };
        let mut d = DrawList::default();
        let dst = Rect::new(10.0, 20.0, 80.0, 40.0);
        d.nine_slice(&ns, dst, 6.0, Color::WHITE);
        assert_eq!(d.quad_count(), 9);
        let area: f32 = d
            .vertices
            .chunks(4)
            .map(|q| (q[1].pos[0] - q[0].pos[0]) * (q[2].pos[1] - q[1].pos[1]))
            .sum();
        assert!((area - dst.w * dst.h).abs() < 1e-3);
        // Top-left corner piece: 6x6 px showing the 6x6 texel corner.
        let q = &d.vertices[0..4];
        assert_eq!(q[0].pos, [10.0, 20.0]);
        assert_eq!(q[2].pos, [16.0, 26.0]);
        assert_eq!(q[0].uv, [100.0, 50.0]);
        assert_eq!(q[2].uv, [106.0, 56.0]);
        // The centre piece shows the middle texels only.
        let c = &d.vertices[16..20];
        assert_eq!(c[0].uv, [106.0, 56.0]);
        assert_eq!(c[2].uv, [110.0, 60.0]);
    }

    #[test]
    fn nine_slice_shrinks_its_corners_for_a_tiny_destination() {
        let ns = NineSlice {
            rect: AtlasRect {
                x: 0,
                y: 0,
                w: 16,
                h: 16,
            },
            border: 6.0,
        };
        let mut d = DrawList::default();
        d.nine_slice(&ns, Rect::new(0.0, 0.0, 8.0, 8.0), 6.0, Color::WHITE);
        let max_x = d.vertices.iter().map(|v| v.pos[0]).fold(0.0, f32::max);
        assert!(max_x <= 8.0);
    }

    #[test]
    fn line_is_a_quad_of_the_right_thickness() {
        let mut d = DrawList::default();
        d.line(WHITE, (0.0, 0.0), (10.0, 0.0), 2.0, Color::WHITE);
        assert_eq!(d.quad_count(), 1);
        let ys: Vec<f32> = d.vertices.iter().map(|v| v.pos[1]).collect();
        assert!(ys.contains(&1.0) && ys.contains(&-1.0));
        d.line(WHITE, (3.0, 3.0), (3.0, 3.0), 2.0, Color::WHITE);
        assert_eq!(d.quad_count(), 1, "a zero-length line draws nothing");
    }

    #[test]
    fn outline_has_four_sides_that_stay_inside() {
        let mut d = DrawList::default();
        let r = Rect::new(5.0, 5.0, 20.0, 10.0);
        d.outline(WHITE, r, 2.0, Color::WHITE);
        assert_eq!(d.quad_count(), 4);
        for v in &d.vertices {
            assert!(v.pos[0] >= r.x && v.pos[0] <= r.right());
            assert!(v.pos[1] >= r.y && v.pos[1] <= r.bottom());
        }
    }
}
