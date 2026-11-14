//! Restyled cutscene parts (WP11.6a): each part of a cutout rig redrawn as an 8-bit RGBA PNG in the
//! style of set v5 — the palette of `model_palette.ron`, two tones per part (light towards the top
//! left, shade towards the bottom right) with a soft third shade between them, and a thin dark
//! outline. The picture has the size of the shape it replaces and is drawn centred on the part's
//! origin like the shape, so the part names, pivots and offsets stay as they are.
//!
//! Integer maths only; the PNG is written uncompressed-deflate by hand (stored blocks), so the
//! bytes are the same everywhere and the pin test can compare them.

/// A shape of a part, in stage units (the cutscene's `Art` without the picture variant).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Rect { w: f32, h: f32 },
    RoundRect { w: f32, h: f32, radius: f32 },
    Circle { radius: f32 },
}

/// Outline width in pixels (thin: one stage unit is about one screen pixel at 1080p).
pub const OUTLINE_PX: i64 = 2;
/// Tone factors in per mille: light, base, soft shade, shade, outline.
pub const LIGHT: i64 = 1100;
pub const SOFT: i64 = 860;
pub const SHADE: i64 = 720;
pub const OUTLINE: i64 = 300;

/// Snap a colour to the nearest palette colour when it is close (squared distance below
/// `SNAP_DIST2`); skin, teeth and other colours far from every swatch keep their own.
pub const SNAP_DIST2: i64 = 40 * 40;

pub fn snap(c: [u8; 3], swatches: &[[u8; 3]]) -> [u8; 3] {
    let d2 = |s: &[u8; 3]| -> i64 {
        (0..3)
            .map(|i| {
                let d = i64::from(c[i]) - i64::from(s[i]);
                d * d
            })
            .sum()
    };
    swatches
        .iter()
        .min_by_key(|s| d2(s))
        .filter(|s| d2(s) < SNAP_DIST2)
        .copied()
        .unwrap_or(c)
}

fn size(v: f32) -> i64 {
    // Stage units to whole pixels, at least 3 so there is an inside.
    (v.ceil() as i64).max(3)
}

/// The RGBA pixels (row-major, top row first) and the size of a part drawn in `colour`.
pub fn draw(shape: Shape, colour: [u8; 3]) -> (u32, u32, Vec<u8>) {
    let (w, h, r) = match shape {
        Shape::Rect { w, h } => (size(w), size(h), 0),
        Shape::RoundRect { w, h, radius } => {
            let (w, h) = (size(w), size(h));
            (w, h, (radius.round() as i64).min(w.min(h) / 2).max(0))
        }
        Shape::Circle { radius } => {
            let d = size(radius * 2.0);
            (d, d, d / 2)
        }
    };
    // Distance (in pixels, squared-free) from a pixel centre to the shape's edge, inside only:
    // work in half pixels so centres are integers.
    let inside_depth = |x: i64, y: i64| -> Option<i64> {
        let (px, py) = (2 * x + 1, 2 * y + 1);
        let (w2, h2, r2) = (2 * w, 2 * h, 2 * r);
        // Distance to the nearest straight edge.
        let straight = px.min(w2 - px).min(py).min(h2 - py);
        if straight < 0 {
            return None;
        }
        // In a corner square: distance to the corner circle.
        let cx = if px < r2 {
            r2
        } else if px > w2 - r2 {
            w2 - r2
        } else {
            px
        };
        let cy = if py < r2 {
            r2
        } else if py > h2 - r2 {
            h2 - r2
        } else {
            py
        };
        if (cx, cy) != (px, py) {
            let (dx, dy) = (px - cx, py - cy);
            let dist = isqrt(dx * dx + dy * dy);
            if dist > r2 {
                return None;
            }
            return Some((r2 - dist) / 2);
        }
        Some(straight / 2)
    };
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let Some(depth) = inside_depth(x, y) else {
                px.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            };
            // Position along the light diagonal, -1000 (top left) to 1000 (bottom right).
            let d = ((2 * x + 1 - w) * 1000 / w + (2 * y + 1 - h) * 1000 / h) / 2;
            let tone = if depth < OUTLINE_PX {
                OUTLINE
            } else if d < -150 {
                LIGHT
            } else if d > 450 {
                SHADE
            } else if d > 250 {
                SOFT
            } else {
                1000
            };
            for c in colour {
                px.push((i64::from(c) * tone / 1000).clamp(0, 255) as u8);
            }
            px.push(255);
        }
    }
    (w as u32, h as u32, px)
}

fn isqrt(v: i64) -> i64 {
    if v <= 0 {
        return 0;
    }
    let mut x = (v as f64).sqrt() as i64;
    while x * x > v {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= v {
        x += 1;
    }
    x
}

/// An 8-bit RGBA PNG of the pixels: zlib with stored (uncompressed) blocks, written by hand.
pub fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + h as usize);
    for row in rgba.chunks_exact(w as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        let len = c.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(c);
    }
    if raw.is_empty() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_in = kind.to_vec();
    crc_in.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
