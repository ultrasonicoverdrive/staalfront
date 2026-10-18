//! PNG read/write and golden-image comparison (screenshot tests).
//!
//! Uses the small pure-Rust `png` crate; we only need 8-bit RGBA.

use std::fmt;
use std::fs;
use std::io::{BufWriter, Cursor};
use std::path::Path;

/// 8-bit RGBA image, row-major, no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba8 {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * 4,
            "pixel buffer size does not match {width}x{height}"
        );
        Self {
            width,
            height,
            pixels,
        }
    }

    /// RGBA of the pixel at `(x, y)`.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}

#[derive(Debug)]
pub enum ImageError {
    Io(std::io::Error),
    Png(String),
    Format(String),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Png(e) => write!(f, "png: {e}"),
            Self::Format(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ImageError {}

impl From<std::io::Error> for ImageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Write an 8-bit RGBA PNG.
pub fn write_png(path: &Path, image: &Rgba8) -> Result<(), ImageError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = BufWriter::new(fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| ImageError::Png(e.to_string()))?;
    writer
        .write_image_data(&image.pixels)
        .map_err(|e| ImageError::Png(e.to_string()))?;
    writer
        .finish()
        .map_err(|e| ImageError::Png(e.to_string()))?;
    Ok(())
}

/// Read a PNG; 8-bit RGBA, RGB, grey and grey with alpha are accepted.
pub fn read_png(path: &Path) -> Result<Rgba8, ImageError> {
    decode_png(&fs::read(path)?)
}

/// Decode the bytes of a PNG; 8-bit RGBA, RGB, grey and grey with alpha are
/// accepted.
pub fn decode_png(bytes: &[u8]) -> Result<Rgba8, ImageError> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| ImageError::Png(e.to_string()))?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| ImageError::Png(e.to_string()))?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(ImageError::Format("only 8-bit PNGs are supported".into()));
    }
    buf.truncate(info.buffer_size());
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        other => {
            return Err(ImageError::Format(format!(
                "unsupported PNG color type {other:?}"
            )));
        }
    };
    Ok(Rgba8::new(info.width, info.height, pixels))
}

/// Allowed deviation between two images.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    /// A pixel counts as differing when any channel differs by more than this.
    pub channel: u8,
    /// Largest allowed share of differing pixels, `0.0..=1.0`.
    pub max_ratio: f64,
}

/// Result of [`compare`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diff {
    pub differing: usize,
    pub total: usize,
    /// Largest per-channel difference over the whole image.
    pub max_channel: u8,
}

impl Diff {
    pub fn ratio(&self) -> f64 {
        self.differing as f64 / self.total.max(1) as f64
    }

    pub fn within(&self, tol: Tolerance) -> bool {
        self.ratio() <= tol.max_ratio
    }
}

/// Compare two images of equal size; `None` when the sizes differ.
pub fn compare(a: &Rgba8, b: &Rgba8, channel_tol: u8) -> Option<Diff> {
    if (a.width, a.height) != (b.width, b.height) {
        return None;
    }
    let mut differing = 0;
    let mut max_channel = 0;
    for (pa, pb) in a.pixels.chunks_exact(4).zip(b.pixels.chunks_exact(4)) {
        let worst = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        max_channel = max_channel.max(worst);
        if worst > channel_tol {
            differing += 1;
        }
    }
    Some(Diff {
        differing,
        total: a.pixels.len() / 4,
        max_channel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Rgba8 {
        Rgba8::new(w, h, rgba.repeat((w * h) as usize))
    }

    #[test]
    fn png_round_trip() {
        let mut img = solid(5, 3, [10, 20, 30, 255]);
        img.pixels[0] = 200;
        let path = std::env::temp_dir().join(format!("sf_render_rt_{}.png", std::process::id()));
        write_png(&path, &img).unwrap();
        let back = read_png(&path).unwrap();
        fs::remove_file(&path).ok();
        assert_eq!(back, img);
    }

    #[test]
    fn compare_counts_pixels_over_tolerance() {
        let a = solid(4, 4, [100, 100, 100, 255]);
        let mut b = a.clone();
        b.pixels[0] = 105; // within 8
        b.pixels[4] = 120; // over 8
        let d = compare(&a, &b, 8).unwrap();
        assert_eq!(d.differing, 1);
        assert_eq!(d.total, 16);
        assert_eq!(d.max_channel, 20);
        assert!(d.within(Tolerance {
            channel: 8,
            max_ratio: 0.1
        }));
        assert!(!d.within(Tolerance {
            channel: 8,
            max_ratio: 0.01
        }));
    }

    #[test]
    fn compare_rejects_size_mismatch() {
        assert!(compare(&solid(2, 2, [0; 4]), &solid(3, 2, [0; 4]), 0).is_none());
    }
}
