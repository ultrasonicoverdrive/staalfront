//! The fog texture of the terrain (WP8.3): what the local player sees and has seen, as two soft maps
//! the terrain shader reads.
//!
//! The simulation keeps two bitsets per player (visible now, explored at some time; bit
//! `y * width + x`, `docs/rules/vision.md`). [`texels`] turns them into an `Rg8` image of one texel per
//! cell: red is *explored*, green is *visible*, each 0 or 255 before softening. Then it runs
//! `softness` passes of a 3 by 3 box blur (two 3-tap passes, clamped at the map edge) over each
//! channel, and the shader's bilinear filter does the rest, so the edge between two states is a ramp of
//! a few cells and not a staircase. Because every texel of *visible* is also *explored* before the blur
//! and the blur is monotone, the blurred green never exceeds the blurred red.
//!
//! Presentation only; it decides nothing. It is rebuilt only when the snapshot's fog version changes.

/// Texel value of a state that holds.
pub const ON: u8 = 255;

/// Whether bit `i` of `words` is set; an index beyond the words is clear.
fn bit(words: &[u64], i: usize) -> bool {
    words.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
}

/// The `Rg8` texels of a `width` by `height` map: red explored, green visible, blurred `softness`
/// times. `2 * width * height` bytes, row-major.
pub fn texels(width: u32, height: u32, visible: &[u64], explored: &[u64], softness: u8) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut red = vec![0u8; w * h];
    let mut green = vec![0u8; w * h];
    for i in 0..w * h {
        if bit(explored, i) {
            red[i] = ON;
        }
        if bit(visible, i) {
            green[i] = ON;
        }
    }
    let mut scratch = vec![0u8; w * h];
    for _ in 0..softness {
        blur(&mut red, &mut scratch, w, h);
        blur(&mut green, &mut scratch, w, h);
    }
    let mut out = Vec::with_capacity(w * h * 2);
    for i in 0..w * h {
        out.push(red[i]);
        out.push(green[i]);
    }
    out
}

/// One pass of the 3 by 3 box blur (horizontal, then vertical, the edge repeats), rounded to nearest.
fn blur(map: &mut [u8], scratch: &mut [u8], w: usize, h: usize) {
    if w == 0 || h == 0 {
        return;
    }
    let third = |a: u8, b: u8, c: u8| ((u16::from(a) + u16::from(b) + u16::from(c) + 1) / 3) as u8;
    for y in 0..h {
        for x in 0..w {
            let at = |x: usize| map[y * w + x];
            scratch[y * w + x] = third(at(x.saturating_sub(1)), at(x), at((x + 1).min(w - 1)));
        }
    }
    for y in 0..h {
        for x in 0..w {
            let at = |y: usize| scratch[y * w + x];
            map[y * w + x] = third(at(y.saturating_sub(1)), at(y), at((y + 1).min(h - 1)));
        }
    }
}

/// An sRGB byte as linear light.
pub fn srgb_to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// An sRGB colour as linear light.
pub fn rgb_to_linear(c: (u8, u8, u8)) -> [f32; 3] {
    [
        srgb_to_linear(c.0),
        srgb_to_linear(c.1),
        srgb_to_linear(c.2),
    ]
}

#[cfg(test)]
mod tests;
