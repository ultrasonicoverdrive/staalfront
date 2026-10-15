//! Rectangles and anchors.
//!
//! Layout is done in two steps: sizes and offsets are given in *logical*
//! pixels (what the designer sees at UI scale 1), [`place`] multiplies them by
//! the effective scale and snaps the result to whole physical pixels so edges
//! stay crisp.

/// An axis-aligned rectangle in pixels, `y` down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    /// Half-open: the left/top edge is inside, the right/bottom edge is not.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Shrunk by `d` on every side (grown when negative); never negative size.
    pub fn inset(&self, d: f32) -> Self {
        Self::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
}

/// The nine anchor points of a rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    /// Horizontal and vertical position of the anchor in `0, 0.5, 1`.
    pub fn factors(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::Top => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::Left => (0.0, 0.5),
            Self::Center => (0.5, 0.5),
            Self::Right => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::Bottom => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }
}

/// Where a box of `size` sits inside `parent`.
///
/// The box's anchor point is attached to the same anchor point of the parent.
/// `offset` is a margin that moves the box *inwards* from an edge anchor (so
/// `(8, 8)` at `BottomRight` leaves 8 px to the right and bottom edges) and
/// is a plain shift (`+x` right, `+y` down) along an axis the anchor centres.
/// `offset` and `size` are in logical pixels; `scale` converts them. The
/// result is in the parent's (physical) pixels, rounded to whole pixels.
pub fn place(
    parent: Rect,
    anchor: Anchor,
    offset: (f32, f32),
    size: (f32, f32),
    scale: f32,
) -> Rect {
    let physical = ((size.0 * scale).round(), (size.1 * scale).round());
    place_physical(parent, anchor, offset, physical, scale)
}

/// [`place`] for a box whose size is already in physical pixels (a measured
/// text, say); only `offset` is scaled.
pub fn place_physical(
    parent: Rect,
    anchor: Anchor,
    offset: (f32, f32),
    size: (f32, f32),
    scale: f32,
) -> Rect {
    let (fx, fy) = anchor.factors();
    let (w, h) = size;
    let inward = |f: f32| if f == 1.0 { -1.0 } else { 1.0 };
    let x = parent.x + fx * (parent.w - w) + inward(fx) * offset.0 * scale;
    let y = parent.y + fy * (parent.h - h) + inward(fy) * offset.1 * scale;
    Rect::new(x.round(), y.round(), w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0.0, 0.0, 1280.0, 720.0);

    #[test]
    fn nine_anchors_without_offset() {
        let size = (100.0, 40.0);
        let at = |a| place(SCREEN, a, (0.0, 0.0), size, 1.0);
        assert_eq!(at(Anchor::TopLeft), Rect::new(0.0, 0.0, 100.0, 40.0));
        assert_eq!(at(Anchor::Top), Rect::new(590.0, 0.0, 100.0, 40.0));
        assert_eq!(at(Anchor::TopRight), Rect::new(1180.0, 0.0, 100.0, 40.0));
        assert_eq!(at(Anchor::Left), Rect::new(0.0, 340.0, 100.0, 40.0));
        assert_eq!(at(Anchor::Center), Rect::new(590.0, 340.0, 100.0, 40.0));
        assert_eq!(at(Anchor::Right), Rect::new(1180.0, 340.0, 100.0, 40.0));
        assert_eq!(at(Anchor::BottomLeft), Rect::new(0.0, 680.0, 100.0, 40.0));
        assert_eq!(at(Anchor::Bottom), Rect::new(590.0, 680.0, 100.0, 40.0));
        assert_eq!(
            at(Anchor::BottomRight),
            Rect::new(1180.0, 680.0, 100.0, 40.0)
        );
    }

    #[test]
    fn offsets_move_inwards_from_edges() {
        let size = (100.0, 40.0);
        let br = place(SCREEN, Anchor::BottomRight, (10.0, 6.0), size, 1.0);
        assert_eq!(br, Rect::new(1170.0, 674.0, 100.0, 40.0));
        let tl = place(SCREEN, Anchor::TopLeft, (10.0, 6.0), size, 1.0);
        assert_eq!(tl, Rect::new(10.0, 6.0, 100.0, 40.0));
        // Centred axes shift plainly; the edge axis still moves inwards.
        let top = place(SCREEN, Anchor::Top, (20.0, 6.0), size, 1.0);
        assert_eq!(top, Rect::new(610.0, 6.0, 100.0, 40.0));
        let right = place(SCREEN, Anchor::Right, (10.0, -20.0), size, 1.0);
        assert_eq!(right, Rect::new(1170.0, 320.0, 100.0, 40.0));
    }

    #[test]
    fn scale_applies_to_size_and_offset_and_rounds_to_pixels() {
        let r = place(SCREEN, Anchor::BottomRight, (8.0, 8.0), (200.0, 150.0), 1.5);
        assert_eq!(
            r,
            Rect::new(1280.0 - 300.0 - 12.0, 720.0 - 225.0 - 12.0, 300.0, 225.0)
        );
        let odd = place(SCREEN, Anchor::TopLeft, (1.0, 1.0), (10.3, 10.3), 1.25);
        assert_eq!(odd.x.fract(), 0.0);
        assert_eq!(odd.w, 13.0);
    }

    #[test]
    fn anchored_to_a_nested_parent() {
        let panel = Rect::new(100.0, 50.0, 400.0, 200.0);
        let inner = place(panel, Anchor::BottomRight, (4.0, 4.0), (60.0, 20.0), 1.0);
        assert_eq!(inner, Rect::new(436.0, 226.0, 60.0, 20.0));
        assert!(panel.contains(inner.x, inner.y));
    }

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(10.0, 10.0, 5.0, 5.0);
        assert!(r.contains(10.0, 10.0));
        assert!(r.contains(14.9, 14.9));
        assert!(!r.contains(15.0, 12.0));
        assert!(!r.contains(12.0, 15.0));
        assert!(!r.contains(9.9, 12.0));
        assert_eq!(r.inset(1.0), Rect::new(11.0, 11.0, 3.0, 3.0));
        assert_eq!(r.inset(9.0).w, 0.0);
    }
}
