//! The procedural skin: a few small grey images in the atlas.
//!
//! Widgets are tinted by the vertex color, so the skin only carries shape
//! and shading: a rounded rectangle with a lighter top, a darker rim and an
//! antialiased edge. Stretching is by nine-slice: the four corners keep their
//! size, the edges stretch along one axis and the middle along both.

use crate::atlas::{Atlas, AtlasRect};

/// Side of the generated square skin images in texels.
const SIZE: u32 = 16;
/// Corner radius and border width of the skin, in texels.
pub const BORDER: f32 = 6.0;

/// An image in the atlas that stretches by nine-slice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NineSlice {
    pub rect: AtlasRect,
    /// Width of the non-stretching border in texels (the same on all sides).
    pub border: f32,
}

/// The skin images and a white texel for solid rectangles.
#[derive(Clone, Copy, Debug)]
pub struct Skin {
    /// Texel `(x, y)` of a fully white, opaque area (solid fills sample here).
    pub white: (f32, f32),
    /// Rounded panel with a thin dark rim.
    pub panel: NineSlice,
    /// Rounded, shaded button face.
    pub button: NineSlice,
}

impl Skin {
    /// Generate the images and pack them into `atlas`.
    pub fn build(atlas: &mut Atlas) -> Self {
        let white = atlas
            .add(2, 2, &[255; 16])
            .expect("a fresh atlas has room for the skin");
        let panel = atlas
            .add(SIZE, SIZE, &rounded(SIZE, 5.0, 0.12, (1.0, 0.92)))
            .expect("room for the panel skin");
        let button = atlas
            .add(SIZE, SIZE, &rounded(SIZE, 5.0, 0.30, (1.0, 0.72)))
            .expect("room for the button skin");
        Self {
            // The middle of the 2x2 block: bilinear sampling stays inside it.
            white: (white.x as f32 + 1.0, white.y as f32 + 1.0),
            panel: NineSlice {
                rect: panel,
                border: BORDER,
            },
            button: NineSlice {
                rect: button,
                border: BORDER,
            },
        }
    }
}

/// A `size` x `size` rounded square: white at the top shading to
/// `gradient.1` at the bottom, a rim (1.3 texels) darkened by `rim`, alpha
/// antialiased at the rounded edge.
fn rounded(size: u32, radius: f32, rim: f32, gradient: (f32, f32)) -> Vec<u8> {
    let s = size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            // Distance inside the rounded box (negative = inside).
            let (cx, cy) = (px.clamp(radius, s - radius), py.clamp(radius, s - radius));
            let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - radius;
            let alpha = (0.5 - d).clamp(0.0, 1.0);
            let rim_t = (1.0 - ((-d - 0.2) / 1.3).clamp(0.0, 1.0)).clamp(0.0, 1.0);
            let t = py / s;
            let shade = gradient.0 + (gradient.1 - gradient.0) * t;
            let v = (shade * (1.0 - rim * rim_t)).clamp(0.0, 1.0);
            let g = (v * 255.0).round() as u8;
            out.extend_from_slice(&[g, g, g, (alpha * 255.0).round() as u8]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skin_packs_into_a_fresh_atlas_with_a_white_texel() {
        let mut atlas = Atlas::default();
        let skin = Skin::build(&mut atlas);
        let (x, y) = (skin.white.0 as u32, skin.white.1 as u32);
        assert_eq!(atlas.texel(x, y), [255, 255, 255, 255]);
        assert_eq!(atlas.texel(x - 1, y - 1), [255, 255, 255, 255]);
    }

    #[test]
    fn rounded_corner_is_transparent_and_centre_is_opaque_and_lighter_than_the_rim() {
        let img = rounded(16, 5.0, 0.3, (1.0, 0.72));
        let at = |x: usize, y: usize| {
            let i = (y * 16 + x) * 4;
            [img[i], img[i + 1], img[i + 2], img[i + 3]]
        };
        assert_eq!(at(0, 0)[3], 0, "outer corner pixel is outside the radius");
        assert_eq!(at(8, 8)[3], 255);
        assert!(
            at(8, 2)[0] > at(8, 0)[0],
            "rim darker than the face below it"
        );
        assert!(
            at(8, 3)[0] > at(8, 12)[0],
            "light at the top, shaded at the bottom"
        );
    }
}
