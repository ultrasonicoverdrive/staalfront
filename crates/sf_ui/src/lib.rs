//! Own UI on wgpu: rectangles, nine-slice skin, text, buttons, anchored
//! panels, UI scale, input that the UI sees first, and the minimap.
//!
//! This is the *player-facing* UI (PLAN.md section 1.0); `egui` stays for
//! debug panels and tools. It is presentation code like `sf_render`: floats
//! are fine here, and the UI never mutates the world; the game turns what a
//! widget reports into commands.
//!
//! # Design
//! * **Immediate mode.** A frame is `begin_frame`, widget calls in painter's
//!   order, `end_frame` ([`Ui`]). A button's `clicked` is the return value of
//!   the call that declares it. Immediate mode needs no widget tree that has
//!   to be kept in sync with the game state (resource counters, build
//!   queues), costs a few microseconds for a HUD, and is simple to test: a
//!   test feeds [`UiInput`] frames and reads responses. Retained mode would
//!   pay off for animated or very large menus; nothing here prevents adding
//!   it on top of the same draw list later.
//! * **One pipeline, two draw calls.** Every primitive is a tinted textured
//!   quad ([`DrawList`]). Solid rectangles sample a white texel, text samples
//!   glyph coverage, panels and buttons use a procedural nine-slice skin; all
//!   of that lives in one dynamically packed atlas ([`Atlas`]). The minimap
//!   is the second texture. [`UiRenderer`] records one pass on top of the
//!   scene through `sf_render::Renderer::render_with`.
//! * **Layout** is nine anchors plus offsets, relative to the window or to
//!   another rectangle ([`layout`]). Sizes and offsets are logical pixels; the
//!   effective scale is the `scale` setting (`assets/config/ui.ron`) times the
//!   window's DPI factor, and results snap to whole physical pixels.
//! * **Input first.** See [`ui`]: presses and wheel over the UI are
//!   consumed, releases always reach the game.
//! * **Text** is rasterized with `ab_glyph` (pure Rust; chosen over `fontdue`
//!   because it is small, pure Rust and exposes kerning)
//!   from a 52 KB subset of Inter (SIL OFL, `assets/fonts/`). There is no
//!   text shaping, right-to-left or font fallback; that is enough for an
//!   English UI with Dutch names and diacritics. See [`text`].
//! * **Minimap** in [`minimap`]: baked terrain texture, unit dots, the
//!   camera's field-of-view frame, click/drag to move the camera.
//!
//! Not in this crate yet: scissor clipping (text must fit its box), scrolling
//! lists, text input, the sidebar and menus (later work packages).

pub mod atlas;
pub mod color;
pub mod draw;
pub mod gpu;
pub mod layout;
pub mod minimap;
pub mod skin;
pub mod text;
pub mod ui;

pub use atlas::{Atlas, AtlasRect};
pub use color::{Color, UiConfig};
pub use draw::{DrawList, Texture};
pub use gpu::UiRenderer;
pub use layout::{Anchor, Rect};
pub use minimap::{Minimap, MinimapGeometry, MinimapImage, clip_to_rect, fov_polygon, fov_quad};
pub use ui::{AreaResponse, MouseButton, Response, Ui, UiInput};
