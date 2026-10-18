//! Software cursors (WP3.5): the pictures of the mouse cursor, drawn by the
//! UI over everything else.
//!
//! The game hides the operating system's cursor while the mouse is over the
//! game view and draws one of these at the mouse position instead, so the
//! cursor can say what a click will do (`docs/rules/cursor.md`). The set is
//! data: `assets/ui/cursors/cursors.ron` names each cursor, its PNG and its
//! hotspot, and the PNGs are packed into the UI's atlas once at start. Drawing
//! one is a single textured quad appended to the UI's draw list after the
//! panels, so it adds no draw call of its own (it joins the atlas batch), and
//! it allocates nothing once the list has seen its largest frame.
//!
//! This crate knows cursors only by name and index; which cursor means what is
//! the game's (`sf_game::cursor`).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;
use sf_render::image;

use crate::atlas::{Atlas, AtlasRect};
use crate::color::Color;
use crate::draw::Texture;
use crate::layout::Rect;
use crate::ui::Ui;

/// Largest `size_px` a cursor set may ask for (logical pixels).
const MAX_SIZE_PX: f32 = 256.0;

/// The file `cursors.ron` as it is written.
#[derive(Deserialize)]
struct File {
    /// Logical pixels the image is drawn at when the UI scale is 1.
    size_px: f32,
    cursors: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
struct Entry {
    /// The PNG, next to `cursors.ron`.
    file: String,
    /// The pixel of the image that is the mouse position, in texels from the
    /// top-left.
    hotspot: (f32, f32),
}

/// Why a cursor set could not be loaded.
#[derive(Debug)]
pub enum CursorError {
    /// `cursors.ron` could not be read or parsed.
    Config(String),
    /// A PNG could not be read, or is not usable.
    Image { name: String, reason: String },
    /// The UI atlas has no room left.
    AtlasFull(String),
}

impl fmt::Display for CursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(e) => write!(f, "cursors.ron: {e}"),
            Self::Image { name, reason } => write!(f, "cursor {name}: {reason}"),
            Self::AtlasFull(name) => write!(f, "cursor {name}: the UI atlas is full"),
        }
    }
}

impl std::error::Error for CursorError {}

/// One cursor picture in the atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorSprite {
    /// The key in `cursors.ron`.
    pub name: String,
    /// Where the picture is in the UI atlas.
    pub rect: AtlasRect,
    /// The mouse position in texels from the top-left of the picture.
    pub hotspot: (f32, f32),
}

/// A set of cursors, packed into the UI atlas. Entries are in the order of
/// their names, so an index is stable for a given file.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorSet {
    size_px: f32,
    sprites: Vec<CursorSprite>,
}

impl CursorSet {
    /// Reads `cursors.ron` and the PNGs in `dir` and packs them into `atlas`.
    /// Nothing is packed when anything is wrong with the set, so a broken
    /// set never leaves half its pictures behind.
    pub fn load(dir: &Path, atlas: &mut Atlas) -> Result<CursorSet, CursorError> {
        let path = dir.join("cursors.ron");
        let text = std::fs::read_to_string(&path)
            .map_err(|e| CursorError::Config(format!("{}: {e}", path.display())))?;
        let file = Self::parse(&text)?;
        let mut images = Vec::with_capacity(file.cursors.len());
        for (name, entry) in &file.cursors {
            let bad = |reason: String| CursorError::Image {
                name: name.clone(),
                reason,
            };
            // A file name only, never a path out of the directory.
            if entry.file.is_empty() || entry.file.contains(['/', '\\']) || entry.file == ".." {
                return Err(bad(format!("{:?} is not a plain file name", entry.file)));
            }
            let picture =
                image::read_png(&dir.join(&entry.file)).map_err(|e| bad(e.to_string()))?;
            check_picture(&picture, entry.hotspot).map_err(bad)?;
            images.push((name.clone(), picture, entry.hotspot));
        }
        let mut sprites = Vec::with_capacity(images.len());
        for (name, picture, hotspot) in images {
            let rect = atlas
                .add(picture.width, picture.height, &picture.pixels)
                .ok_or_else(|| CursorError::AtlasFull(name.clone()))?;
            sprites.push(CursorSprite {
                name,
                rect,
                hotspot,
            });
        }
        Ok(CursorSet {
            size_px: file.size_px,
            sprites,
        })
    }

    fn parse(text: &str) -> Result<File, CursorError> {
        let file: File = ron::from_str(text).map_err(|e| CursorError::Config(e.to_string()))?;
        if !(file.size_px > 0.0 && file.size_px <= MAX_SIZE_PX) {
            return Err(CursorError::Config(format!(
                "size_px {} is not in 0..={MAX_SIZE_PX}",
                file.size_px
            )));
        }
        if file.cursors.is_empty() {
            return Err(CursorError::Config("no cursors".to_owned()));
        }
        Ok(file)
    }

    /// Logical pixels a picture is drawn at when the UI scale is 1.
    pub fn size_px(&self) -> f32 {
        self.size_px
    }

    /// The number of cursors.
    pub fn len(&self) -> usize {
        self.sprites.len()
    }

    /// True if the set has no cursor (a loaded set never is).
    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }

    /// The index of the cursor called `name`.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.sprites.iter().position(|s| s.name == name)
    }

    /// The cursor at `index`.
    pub fn sprite(&self, index: usize) -> Option<&CursorSprite> {
        self.sprites.get(index)
    }

    /// The screen rectangle of cursor `index` when the mouse is at `at`
    /// (physical pixels) and the UI scale is `scale`: its hotspot lands on
    /// `at`, and the corner snaps to whole pixels so that a 1:1 picture keeps
    /// its texels crisp.
    pub fn rect_at(&self, index: usize, at: (f32, f32), scale: f32) -> Option<Rect> {
        let sprite = self.sprites.get(index)?;
        let side = sprite.rect.w as f32;
        let k = self.size_px * scale / side;
        Some(Rect::new(
            (at.0 - sprite.hotspot.0 * k).round(),
            (at.1 - sprite.hotspot.1 * k).round(),
            (side * k).round().max(1.0),
            (sprite.rect.h as f32 * k).round().max(1.0),
        ))
    }
}

/// A cursor picture must be square and the hotspot inside it.
fn check_picture(picture: &image::Rgba8, hotspot: (f32, f32)) -> Result<(), String> {
    if picture.width != picture.height {
        return Err(format!(
            "{}x{} is not square",
            picture.width, picture.height
        ));
    }
    let side = picture.width as f32;
    if !(0.0..side).contains(&hotspot.0) || !(0.0..side).contains(&hotspot.1) {
        return Err(format!(
            "hotspot {hotspot:?} is outside the {}x{} picture",
            picture.width, picture.height
        ));
    }
    Ok(())
}

impl Ui {
    /// Draws cursor `index` of `set` with its hotspot at `at` (physical
    /// pixels), over everything the UI has drawn so far: call it last in the
    /// frame. Does not block input. An index the set does not have draws
    /// nothing.
    pub fn draw_cursor(&mut self, set: &CursorSet, index: usize, at: (f32, f32)) {
        let Some(rect) = set.rect_at(index, at, self.scale()) else {
            return;
        };
        let Some(sprite) = set.sprite(index) else {
            return;
        };
        let r = sprite.rect;
        let uv = [
            r.x as f32,
            r.y as f32,
            (r.x + r.w) as f32,
            (r.y + r.h) as f32,
        ];
        self.list.quad(Texture::Sprites, rect, uv, Color::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::UiConfig;
    use crate::ui::UiInput;

    /// A tiny RGBA PNG, written to a scratch directory.
    fn write_png(path: &Path, side: u32, rgba: [u8; 4]) {
        let pixels: Vec<u8> = rgba.repeat((side * side) as usize);
        image::write_png(path, &image::Rgba8::new(side, side, pixels)).unwrap();
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sf_ui_cursor_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set_in(dir: &Path, ron: &str) -> Result<CursorSet, CursorError> {
        std::fs::write(dir.join("cursors.ron"), ron).unwrap();
        CursorSet::load(dir, &mut Atlas::default())
    }

    const TWO: &str = r#"(size_px: 32, cursors: {
        "b": (file: "b.png", hotspot: (16, 16)),
        "a": (file: "a.png", hotspot: (2, 3)),
    })"#;

    #[test]
    fn a_set_loads_in_name_order_with_its_hotspots() {
        let dir = scratch("load");
        write_png(&dir.join("a.png"), 32, [255, 0, 0, 255]);
        write_png(&dir.join("b.png"), 32, [0, 255, 0, 255]);
        let mut atlas = Atlas::default();
        std::fs::write(dir.join("cursors.ron"), TWO).unwrap();
        let set = CursorSet::load(&dir, &mut atlas).unwrap();
        assert_eq!(set.len(), 2);
        assert_eq!(set.size_px(), 32.0);
        assert_eq!(set.index_of("a"), Some(0));
        assert_eq!(set.index_of("b"), Some(1));
        assert_eq!(set.index_of("c"), None);
        assert_eq!(set.sprite(0).unwrap().hotspot, (2.0, 3.0));
        // The pixels are in the atlas.
        let r = set.sprite(1).unwrap().rect;
        assert_eq!(atlas.texel(r.x, r.y), [0, 255, 0, 255]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_set_is_refused_and_leaves_the_atlas_alone() {
        let dir = scratch("broken");
        write_png(&dir.join("a.png"), 32, [255, 0, 0, 255]);
        // b.png is missing: nothing may be packed.
        std::fs::write(dir.join("cursors.ron"), TWO).unwrap();
        let mut atlas = Atlas::default();
        let before = atlas.revision();
        let err = CursorSet::load(&dir, &mut atlas).unwrap_err();
        assert!(err.to_string().contains("cursor b"), "{err}");
        assert_eq!(atlas.revision(), before, "a half set was packed");
        // A hotspot outside the picture, a bad size, a path as file name.
        write_png(&dir.join("b.png"), 32, [0, 255, 0, 255]);
        for (ron, needle) in [
            (
                r#"(size_px: 32, cursors: {"a": (file: "a.png", hotspot: (40, 3))})"#,
                "hotspot",
            ),
            (
                r#"(size_px: 0, cursors: {"a": (file: "a.png", hotspot: (1, 3))})"#,
                "size_px",
            ),
            (
                r#"(size_px: 32, cursors: {"a": (file: "../a.png", hotspot: (1, 3))})"#,
                "plain file name",
            ),
            (r"(size_px: 32, cursors: {})", "no cursors"),
            ("(oops", "cursors.ron"),
        ] {
            let err = set_in(&dir, ron).unwrap_err().to_string();
            assert!(err.contains(needle), "{ron}: {err}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hotspot_lands_on_the_mouse_and_the_size_follows_the_ui_scale() {
        let dir = scratch("rect");
        write_png(&dir.join("a.png"), 32, [255, 0, 0, 255]);
        write_png(&dir.join("b.png"), 32, [0, 255, 0, 255]);
        let set = set_in(&dir, TWO).unwrap();
        // "a": hotspot (2, 3), 32 px at scale 1 is 1:1.
        let r = set.rect_at(0, (100.0, 50.0), 1.0).unwrap();
        assert_eq!(r, Rect::new(98.0, 47.0, 32.0, 32.0));
        // At scale 2 the picture and the hotspot offset double.
        let r = set.rect_at(0, (100.0, 50.0), 2.0).unwrap();
        assert_eq!(r, Rect::new(96.0, 44.0, 64.0, 64.0));
        // Whatever the scale, the hotspot texel's position is the mouse.
        for scale in [1.0_f32, 1.25, 1.5, 2.0, 3.0] {
            let r = set.rect_at(1, (200.0, 100.0), scale).unwrap();
            let middle = (r.x + r.w * 0.5, r.y + r.h * 0.5);
            assert!(
                (middle.0 - 200.0).abs() <= 1.0 && (middle.1 - 100.0).abs() <= 1.0,
                "scale {scale}: {r:?}"
            );
        }
        assert!(set.rect_at(9, (0.0, 0.0), 1.0).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drawing_a_cursor_adds_one_quad_last_in_its_own_batch_and_blocks_nothing() {
        let dir = scratch("draw");
        write_png(&dir.join("a.png"), 32, [255, 0, 0, 255]);
        write_png(&dir.join("b.png"), 32, [0, 255, 0, 255]);
        std::fs::write(dir.join("cursors.ron"), TWO).unwrap();
        let mut ui = Ui::new(UiConfig::default());
        let set = CursorSet::load(&dir, ui.atlas_mut()).unwrap();
        ui.begin_frame(UiInput::default(), (800, 600), 1.0);
        let bar = ui.panel(crate::layout::Anchor::TopLeft, (0.0, 0.0), (200.0, 40.0));
        ui.end_frame();
        let quads = ui.draw_list().quad_count();
        ui.draw_cursor(&set, 1, (30.0, 20.0));
        assert_eq!(ui.draw_list().quad_count(), quads + 1);
        // It is the last thing in the list (over the panel), in a batch of
        // its own: the colours of a sprite are read as sRGB data.
        let list = ui.draw_list();
        assert_eq!(list.batches.len(), 2);
        assert_eq!(list.batches[0].texture, Texture::Atlas);
        assert_eq!(list.batches[1].texture, Texture::Sprites);
        let r = set.sprite(1).unwrap().rect;
        assert_eq!(
            list.vertices[list.vertices.len() - 4].uv,
            [r.x as f32, r.y as f32]
        );
        // Another cursor joins that batch.
        ui.draw_cursor(&set, 0, (60.0, 20.0));
        assert_eq!(ui.draw_list().batches.len(), 2);
        assert_eq!(ui.draw_list().quad_count(), quads + 2);
        // It blocks no input: the same frame's blockers are unchanged.
        assert!(!ui.blocks_pointer_at(600.0, 500.0));
        assert!(bar.contains(30.0, 20.0));
        // An unknown index draws nothing.
        ui.draw_cursor(&set, 99, (30.0, 20.0));
        assert_eq!(ui.draw_list().quad_count(), quads + 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drawing_cursors_does_not_allocate_once_the_list_has_grown() {
        let dir = scratch("alloc");
        write_png(&dir.join("a.png"), 32, [255, 0, 0, 255]);
        write_png(&dir.join("b.png"), 32, [0, 255, 0, 255]);
        std::fs::write(dir.join("cursors.ron"), TWO).unwrap();
        let mut ui = Ui::new(UiConfig::default());
        let set = CursorSet::load(&dir, ui.atlas_mut()).unwrap();
        let frame = |ui: &mut Ui| {
            ui.begin_frame(UiInput::default(), (800, 600), 1.0);
            ui.end_frame();
            ui.draw_cursor(&set, 0, (10.0, 10.0));
        };
        frame(&mut ui);
        let (v, i) = (
            ui.draw_list().vertices.capacity(),
            ui.draw_list().indices.capacity(),
        );
        for _ in 0..100 {
            frame(&mut ui);
        }
        assert_eq!(
            (
                ui.draw_list().vertices.capacity(),
                ui.draw_list().indices.capacity()
            ),
            (v, i)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_shipped_set_loads() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/ui/cursors");
        let set = CursorSet::load(&dir, &mut Atlas::default()).unwrap();
        assert_eq!(set.len(), 24);
        assert!(set.index_of("select").is_some());
        assert_eq!(set.size_px(), 32.0);
    }
}
