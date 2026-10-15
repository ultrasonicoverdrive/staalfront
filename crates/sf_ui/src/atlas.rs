//! The texture atlas: dynamic shelf packing of glyph bitmaps and the skin.
//!
//! The atlas is a CPU RGBA8 image with a fixed width that grows downwards
//! (the height doubles up to [`MAX_HEIGHT`]). Everything the UI draws that is
//! not the minimap lives in it, so the whole UI is one texture and one draw
//! call. The GPU copy is refreshed by `UiRenderer` when the revision moved.
//!
//! Packing is by shelves: a bitmap goes on the first shelf that is tall enough
//! (the tightest fit) and has room; otherwise a new shelf is opened below the
//! last one. Every bitmap is surrounded by a 1 px transparent margin so that
//! bilinear sampling never bleeds a neighbour in.

/// Margin around each packed bitmap, in texels.
pub const PAD: u32 = 1;
/// Atlas width in texels (fixed).
pub const WIDTH: u32 = 512;
/// Height a new atlas starts with.
pub const START_HEIGHT: u32 = 128;
/// Tallest the atlas may grow; beyond that packing fails.
pub const MAX_HEIGHT: u32 = 2048;

/// A packed bitmap: position and size in texels (without the margin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy, Debug)]
struct Shelf {
    y: u32,
    height: u32,
    /// Next free x on this shelf.
    used: u32,
}

/// Shelf packer for a `width` x growing-height area. No pixels, so it is
/// testable (and fast) on its own.
#[derive(Clone, Debug)]
pub struct ShelfPacker {
    width: u32,
    height: u32,
    max_height: u32,
    shelves: Vec<Shelf>,
}

impl ShelfPacker {
    pub fn new(width: u32, height: u32, max_height: u32) -> Self {
        Self {
            width,
            height,
            max_height: max_height.max(height),
            shelves: Vec::new(),
        }
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Reserve room for a `w` x `h` bitmap (margin added here). `None` when
    /// it cannot fit even after growing to the maximum height.
    pub fn alloc(&mut self, w: u32, h: u32) -> Option<AtlasRect> {
        let (pw, ph) = (w + 2 * PAD, h + 2 * PAD);
        if pw > self.width {
            return None;
        }
        loop {
            if let Some(rect) = self.try_alloc(pw, ph) {
                return Some(AtlasRect {
                    x: rect.0 + PAD,
                    y: rect.1 + PAD,
                    w,
                    h,
                });
            }
            if self.height >= self.max_height {
                return None;
            }
            self.height = (self.height * 2).min(self.max_height);
        }
    }

    fn try_alloc(&mut self, pw: u32, ph: u32) -> Option<(u32, u32)> {
        let best = self
            .shelves
            .iter_mut()
            .filter(|s| s.height >= ph && self.width - s.used >= pw)
            .min_by_key(|s| s.height);
        if let Some(shelf) = best {
            let at = (shelf.used, shelf.y);
            shelf.used += pw;
            return Some(at);
        }
        let y = self.shelves.last().map_or(0, |s| s.y + s.height);
        if y + ph > self.height {
            return None;
        }
        self.shelves.push(Shelf {
            y,
            height: ph,
            used: pw,
        });
        Some((0, y))
    }
}

/// The atlas image plus its packer. Every change bumps [`Atlas::revision`];
/// a renderer remembers the revision it uploaded, so any number of renderers
/// (and a renderer created later) stay correct. The whole image is uploaded
/// when the revision moved, which only happens while new glyphs are first
/// seen.
#[derive(Clone, Debug)]
pub struct Atlas {
    packer: ShelfPacker,
    /// `WIDTH * packer.height()` RGBA8 texels.
    pixels: Vec<u8>,
    revision: u64,
}

impl Default for Atlas {
    fn default() -> Self {
        Self::with_limits(START_HEIGHT, MAX_HEIGHT)
    }
}

impl Atlas {
    pub fn with_limits(start_height: u32, max_height: u32) -> Self {
        Self {
            packer: ShelfPacker::new(WIDTH, start_height, max_height),
            pixels: vec![0; (WIDTH * start_height * 4) as usize],
            revision: 0,
        }
    }

    pub fn width(&self) -> u32 {
        WIDTH
    }

    pub fn height(&self) -> u32 {
        self.packer.height()
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Counts the bitmaps added so far; changes whenever the pixels do.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Pack an RGBA8 bitmap (`w * h * 4` bytes). `None` when the atlas is full.
    pub fn add(&mut self, w: u32, h: u32, rgba: &[u8]) -> Option<AtlasRect> {
        assert_eq!(rgba.len(), (w * h * 4) as usize, "bitmap size mismatch");
        let rect = self.packer.alloc(w, h)?;
        let needed = (WIDTH * self.packer.height() * 4) as usize;
        if needed > self.pixels.len() {
            self.pixels.resize(needed, 0);
        }
        for row in 0..h {
            let dst = (((rect.y + row) * WIDTH + rect.x) * 4) as usize;
            let src = (row * w * 4) as usize;
            self.pixels[dst..dst + (w * 4) as usize]
                .copy_from_slice(&rgba[src..src + (w * 4) as usize]);
        }
        self.revision += 1;
        Some(rect)
    }

    /// The texel at `(x, y)`.
    pub fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * WIDTH + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlaps(a: &AtlasRect, b: &AtlasRect) -> bool {
        let pad = PAD;
        a.x < b.x + b.w + pad
            && b.x < a.x + a.w + pad
            && a.y < b.y + b.h + pad
            && b.y < a.y + a.h + pad
    }

    #[test]
    fn packs_without_overlap_and_inside_bounds() {
        let mut p = ShelfPacker::new(256, 64, 1024);
        let mut rects = Vec::new();
        for i in 0..300u32 {
            let (w, h) = (6 + i % 11, 8 + (i * 7) % 13);
            let r = p.alloc(w, h).expect("room");
            assert!(r.x >= PAD && r.x + r.w + PAD <= 256);
            assert!(r.y + r.h + PAD <= p.height());
            rects.push(r);
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!overlaps(a, b), "{a:?} overlaps {b:?}");
            }
        }
        assert!(p.height() > 64, "the atlas grew");
    }

    #[test]
    fn same_height_bitmaps_share_a_shelf() {
        let mut p = ShelfPacker::new(100, 64, 64);
        let a = p.alloc(10, 10).unwrap();
        let b = p.alloc(10, 10).unwrap();
        assert_eq!(a.y, b.y);
        assert_eq!(b.x, a.x + a.w + 2 * PAD);
    }

    #[test]
    fn a_full_atlas_returns_none_without_panicking() {
        let mut p = ShelfPacker::new(64, 32, 64);
        let mut got = 0;
        while p.alloc(20, 20).is_some() {
            got += 1;
            assert!(got < 100, "must run out");
        }
        assert!(got >= 2);
        assert!(p.alloc(200, 10).is_none(), "wider than the atlas");
        assert_eq!(p.height(), 64);
    }

    #[test]
    fn atlas_copies_pixels_and_bumps_the_revision() {
        let mut a = Atlas::with_limits(32, 256);
        assert_eq!(a.revision(), 0);
        let bitmap = [9, 8, 7, 6].repeat(4 * 3);
        let r = a.add(4, 3, &bitmap).unwrap();
        assert_eq!(a.revision(), 1);
        assert_eq!(a.texel(r.x, r.y), [9, 8, 7, 6]);
        assert_eq!(a.texel(r.x + 3, r.y + 2), [9, 8, 7, 6]);
        assert_eq!(
            a.texel(r.x - 1, r.y),
            [0, 0, 0, 0],
            "margin stays transparent"
        );
    }

    #[test]
    fn growing_keeps_old_pixels_and_resizes_the_image() {
        let mut a = Atlas::with_limits(16, 128);
        let first = a.add(8, 8, &[1, 2, 3, 4].repeat(64)).unwrap();
        a.add(8, 12, &[5, 5, 5, 5].repeat(96)).unwrap();
        a.add(8, 12, &[5, 5, 5, 5].repeat(96)).unwrap();
        assert!(a.height() > 16);
        assert_eq!(a.pixels().len(), (a.width() * a.height() * 4) as usize);
        assert_eq!(a.texel(first.x, first.y), [1, 2, 3, 4]);
    }
}
