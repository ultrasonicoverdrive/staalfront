//! Pictures for image parts and layers: read from the asset root once, kept
//! as pixels (the GPU copy is made by the renderer the first frame it is
//! drawn).
//!
//! Only 8-bit RGB and RGBA PNG files are read (the reader of `sf_render`);
//! SVG, palette and grey PNGs count as "cannot load" and are drawn as a
//! placeholder box like a missing file. The scene validator already refuses
//! paths that leave the asset root; this is the second lock on the same door.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Largest picture side that is accepted (the default wgpu texture limit).
pub const MAX_SIDE: u32 = 8192;

/// A decoded picture.
#[derive(Clone, Debug)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// RGBA8, straight alpha, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

/// A picture the plan can draw: its index in the store and its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PictureRef {
    pub index: usize,
    pub width: u32,
    pub height: u32,
}

enum Entry {
    Loaded(usize),
    Failed(String),
}

/// Loads each picture once, by the path as written in the scene.
pub struct ImageStore {
    root: PathBuf,
    entries: BTreeMap<String, Entry>,
    pictures: Vec<Picture>,
}

impl ImageStore {
    /// A store reading files below `root` (the asset root, `assets/`).
    pub fn new(root: impl Into<PathBuf>) -> ImageStore {
        ImageStore {
            root: root.into(),
            entries: BTreeMap::new(),
            pictures: Vec::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The picture at `path`, loading it on first use. `None` if it does not
    /// exist or cannot be read (see [`ImageStore::failures`] for why).
    pub fn get(&mut self, path: &str) -> Option<PictureRef> {
        if !self.entries.contains_key(path) {
            let entry = match self.load(path) {
                Ok(picture) => {
                    self.pictures.push(picture);
                    Entry::Loaded(self.pictures.len() - 1)
                }
                Err(why) => Entry::Failed(why),
            };
            self.entries.insert(path.to_string(), entry);
        }
        match self.entries.get(path) {
            Some(Entry::Loaded(i)) => {
                let p = &self.pictures[*i];
                Some(PictureRef {
                    index: *i,
                    width: p.width,
                    height: p.height,
                })
            }
            _ => None,
        }
    }

    /// Put a picture in the store under `path` without touching the file
    /// system (tests, generated art).
    pub fn insert(&mut self, path: &str, picture: Picture) {
        self.pictures.push(picture);
        self.entries
            .insert(path.to_string(), Entry::Loaded(self.pictures.len() - 1));
    }

    /// Forget every picture, loaded or failed, so the next [`ImageStore::get`]
    /// reads the files again (live preview after a file changed). Earlier
    /// [`PictureRef`]s are void afterwards.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.pictures.clear();
    }

    /// The decoded picture behind a [`PictureRef`].
    pub fn picture(&self, index: usize) -> &Picture {
        &self.pictures[index]
    }

    /// Paths that were asked for and could not be loaded, with the reason.
    pub fn failures(&self) -> Vec<(&str, &str)> {
        self.entries
            .iter()
            .filter_map(|(k, v)| match v {
                Entry::Failed(why) => Some((k.as_str(), why.as_str())),
                Entry::Loaded(_) => None,
            })
            .collect()
    }

    fn load(&self, path: &str) -> Result<Picture, String> {
        let rel = Path::new(path);
        let escapes = rel.is_absolute()
            || path.contains('\\')
            || rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir));
        if escapes {
            return Err("path leaves the asset root".into());
        }
        if path.to_ascii_lowercase().ends_with(".svg") {
            return Err("SVG is not supported by the player yet; export a PNG".into());
        }
        let full = self.root.join(rel);
        if !full.is_file() {
            return Err("file not found".into());
        }
        let img = sf_render::image::read_png(&full).map_err(|e| e.to_string())?;
        if img.width == 0 || img.height == 0 || img.width > MAX_SIDE || img.height > MAX_SIDE {
            return Err(format!(
                "picture size {}x{} is outside 1..={MAX_SIDE}",
                img.width, img.height
            ));
        }
        Ok(Picture {
            width: img.width,
            height: img.height,
            pixels: img.pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> Picture {
        Picture {
            width: 2,
            height: 1,
            pixels: vec![255; 8],
        }
    }

    #[test]
    fn inserted_pictures_are_found_and_counted_once() {
        let mut s = ImageStore::new("nowhere");
        s.insert("a.png", tiny());
        let r = s.get("a.png").expect("inserted");
        assert_eq!((r.width, r.height), (2, 1));
        assert_eq!(s.get("a.png"), Some(r));
        assert_eq!(s.picture(r.index).pixels.len(), 8);
        assert!(s.failures().is_empty());
    }

    #[test]
    fn clear_forgets_pictures_and_failures() {
        let mut s = ImageStore::new("nowhere");
        s.insert("a.png", tiny());
        assert!(s.get("b.png").is_none());
        s.clear();
        assert!(s.failures().is_empty());
        assert!(
            s.get("a.png").is_none(),
            "gone; asked again it is read from disk"
        );
        assert_eq!(s.failures().len(), 1);
    }

    #[test]
    fn missing_and_unsupported_files_fail_with_a_reason() {
        let mut s = ImageStore::new("nowhere");
        assert!(s.get("art/none.png").is_none());
        assert!(s.get("art/x.svg").is_none());
        assert!(s.get("../secret.png").is_none());
        assert!(s.get("/etc/passwd").is_none());
        let f = s.failures();
        assert_eq!(f.len(), 4);
        assert!(
            f.iter()
                .any(|(p, why)| *p == "art/none.png" && why.contains("not found"))
        );
        assert!(
            f.iter()
                .any(|(p, why)| *p == "art/x.svg" && why.contains("SVG"))
        );
        assert!(
            f.iter()
                .any(|(p, why)| *p == "../secret.png" && why.contains("leaves"))
        );
    }
}
