//! From a [`SceneState`] to a list of quads: everything about *what is drawn
//! where*, with no GPU involved (so it is unit-tested).
//!
//! A frame is painter's order, back to front:
//!
//! 1. the picture area (stage fitted into the target, letterboxed), clipped by
//!    a scissor rectangle:
//!    background layers (`z < 0`, parallax already in their position),
//!    characters (parts in their `z` order), foreground layers (`z >= 0`),
//!    the map overlay;
//! 2. over the whole target: the fade, then the subtitle, then the optional
//!    [`Hud`] of the viewer.
//!
//! Every primitive is one quad of [`Vertex`]es. Shapes (rectangles, rounded
//! rectangles, circles) carry their own coordinates and are anti-aliased in
//! the fragment shader from a signed distance, so rotated cutout parts have
//! smooth edges without multisampling. Pictures and glyphs are textured
//! quads; the fade and the bars are flat colour.

use bytemuck::{Pod, Zeroable};

use crate::math::{Affine, Color, Vec2};
use crate::model::{Art, Delivery, Stage};
use crate::sample::{CameraState, MapOverlayState, SceneState, Subtitle};

use super::images::ImageStore;
use super::text::{SubtitleLayout, TextEngine, layout_subtitle};

pub const KIND_SHAPE: f32 = 0.0;
pub const KIND_TEXTURE: f32 = 1.0;
pub const KIND_SOLID: f32 = 2.0;

/// One corner of a quad. Mirrors `VertexIn` in `assets/shaders/cutscene.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// Target pixels.
    pub pos: [f32; 2],
    /// Texels of the bound texture (textured quads).
    pub uv: [f32; 2],
    /// Position inside the shape in its own units (shapes).
    pub local: [f32; 2],
    /// Half width, half height, corner radius, pixels per own unit.
    pub params: [f32; 4],
    pub kind: f32,
    /// sRGB.
    pub color: [u8; 4],
}

/// Which texture a batch samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexId {
    /// A 1x1 white texel: shapes and flat colour.
    White,
    /// The glyph atlas.
    Glyphs,
    /// A picture of the [`ImageStore`] by index.
    Image(usize),
}

/// A pixel rectangle on the target (for the scissor test).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Quads drawn with the same texture and scissor, as a range of indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    pub tex: TexId,
    pub scissor: PixelRect,
    pub start: u32,
    pub end: u32,
}

/// A rectangle in target pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn centre(&self) -> Vec2 {
        Vec2::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// Playback information for the viewer's overlay: the progress bar along the
/// bottom, and (live preview, WP12.6) text panels stacked down the left edge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hud {
    /// `0..=1` along the scene.
    pub progress: f32,
    /// Where the shots start, `0..=1`.
    pub shot_marks: Vec<f32>,
    pub paused: bool,
    /// Text boxes, top left, first one on top. Empty = no text at all.
    pub panels: Vec<HudPanel>,
}

/// How a panel or one of its lines looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudTone {
    /// White on dark blue.
    Normal,
    /// Grey-blue: secondary information.
    Dim,
    /// Amber: something is not as intended, but the scene plays.
    Warning,
    /// Red: something is wrong and was not applied.
    Error,
}

/// One line of text in a [`HudPanel`]; wrapped to the window if it is long.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HudLine {
    pub text: String,
    pub tone: HudTone,
}

impl HudLine {
    pub fn new(text: impl Into<String>, tone: HudTone) -> HudLine {
        HudLine {
            text: text.into(),
            tone,
        }
    }
}

/// A box of text over the picture (the writer's HUD, the error list). The
/// tone sets the colour of the box; each line has a tone of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HudPanel {
    pub tone: HudTone,
    pub lines: Vec<HudLine>,
}

/// Where things ended up, for tests and for the caller (the screenshot test
/// looks for text pixels inside `subtitle`).
#[derive(Clone, Debug, PartialEq)]
pub struct FrameLayout {
    pub target: (u32, u32),
    /// The picture (stage) inside the target.
    pub viewport: Rect,
    pub subtitle: Option<SubtitleLayout>,
}

/// The quads of one frame.
#[derive(Clone, Debug)]
pub struct FramePlan {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<Batch>,
    pub layout: FrameLayout,
}

/// What building a plan needs besides the state: the text engine and the
/// pictures.
pub struct Resources {
    pub text: TextEngine,
    pub images: ImageStore,
    /// The last subtitle layout with what it was made for: a line stays up
    /// for seconds, and wrapping it is the most expensive part of a frame.
    subtitle: Option<(SubtitleKey, SubtitleLayout)>,
}

/// What a [`SubtitleLayout`] depends on.
#[derive(Clone, Debug, PartialEq)]
struct SubtitleKey {
    name: String,
    text: String,
    viewport: [u32; 4],
}

impl Resources {
    pub fn new(asset_root: impl Into<std::path::PathBuf>) -> Resources {
        Resources {
            text: TextEngine::default(),
            images: ImageStore::new(asset_root),
            subtitle: None,
        }
    }

    /// The layout of this line in this picture, reused while nothing changes.
    fn subtitle_layout(&mut self, name: &str, text: &str, vp: &Rect) -> SubtitleLayout {
        let key = SubtitleKey {
            name: name.to_string(),
            text: text.to_string(),
            viewport: [vp.x as u32, vp.y as u32, vp.w as u32, vp.h as u32],
        };
        if let Some((k, layout)) = &self.subtitle
            && *k == key
        {
            return layout.clone();
        }
        let layout = layout_subtitle(&self.text, name, text, vp);
        self.subtitle = Some((key, layout.clone()));
        layout
    }
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// The stage fitted into a `target` of pixels, centred, whole picture visible
/// (bars at the sides or at the top and bottom when the shapes differ).
pub fn fit_viewport(target: (u32, u32), stage: Stage) -> Rect {
    let (tw, th) = (target.0.max(1) as f32, target.1.max(1) as f32);
    let (sw, sh) = (stage.width.max(1) as f32, stage.height.max(1) as f32);
    let k = (tw / sw).min(th / sh);
    let (w, h) = ((sw * k).round(), (sh * k).round());
    Rect {
        x: ((tw - w) / 2.0).round(),
        y: ((th - h) / 2.0).round(),
        w,
        h,
    }
}

/// World (stage units) to target pixels: the camera's pan, zoom and shake and
/// the fit of the stage into the viewport.
pub fn view_affine(vp: &Rect, stage: Stage, cam: &CameraState) -> Affine {
    let k = vp.w / stage.width.max(1) as f32 * cam.zoom;
    Affine::translation(vp.centre())
        .then_after(&Affine::scaling(Vec2::new(k, k)))
        .then_after(&Affine::translation(cam.shake - cam.center))
}

/// Stage units to target pixels without any camera (the map overlay, which is
/// an insert glued to the frame).
fn stage_affine(vp: &Rect, stage: Stage) -> Affine {
    let k = vp.w / stage.width.max(1) as f32;
    Affine::translation(vp.centre()).then_after(&Affine::scaling(Vec2::new(k, k)))
}

fn scissor_of(r: &Rect, target: (u32, u32)) -> PixelRect {
    let x0 = r.x.floor().clamp(0.0, target.0 as f32);
    let y0 = r.y.floor().clamp(0.0, target.1 as f32);
    let x1 = r.right().ceil().clamp(0.0, target.0 as f32);
    let y1 = r.bottom().ceil().clamp(0.0, target.1 as f32);
    PixelRect {
        x: x0 as u32,
        y: y0 as u32,
        w: (x1 - x0).max(0.0) as u32,
        h: (y1 - y0).max(0.0) as u32,
    }
}

fn rgba(c: Color) -> [u8; 4] {
    [c.r, c.g, c.b, c.a]
}

fn with_alpha(c: Color, alpha: f32) -> Color {
    Color {
        a: (f32::from(c.a) * alpha.clamp(0.0, 1.0)).round() as u8,
        ..c
    }
}

/// Drawn where a picture could not be loaded (about the size of a figure's
/// hand, so it is noticed but does not take over the picture).
const MISSING_COLOR: Color = Color {
    r: 255,
    g: 0,
    b: 255,
    a: 160,
};
const MISSING_HALF: f32 = 64.0;

// ---------------------------------------------------------------------------
// Painter
// ---------------------------------------------------------------------------

struct Painter {
    scissor: PixelRect,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    batches: Vec<Batch>,
}

impl Painter {
    fn new(target: (u32, u32)) -> Painter {
        Painter {
            scissor: PixelRect {
                x: 0,
                y: 0,
                w: target.0,
                h: target.1,
            },
            vertices: Vec::new(),
            indices: Vec::new(),
            batches: Vec::new(),
        }
    }

    fn set_scissor(&mut self, r: PixelRect) {
        self.scissor = r;
    }

    /// Whether the pixel bounding box of `pts` touches the current scissor.
    fn visible(&self, pts: &[[f32; 2]; 4]) -> bool {
        if pts.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
            return false;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in pts {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        let s = self.scissor;
        x1 >= s.x as f32 && y1 >= s.y as f32 && x0 <= (s.x + s.w) as f32 && y0 <= (s.y + s.h) as f32
    }

    fn quad(&mut self, tex: TexId, corners: [Vertex; 4]) {
        let pts = [
            corners[0].pos,
            corners[1].pos,
            corners[2].pos,
            corners[3].pos,
        ];
        if !self.visible(&pts) {
            return;
        }
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&corners);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        let end = self.indices.len() as u32;
        match self.batches.last_mut() {
            Some(b) if b.tex == tex && b.scissor == self.scissor => b.end = end,
            _ => self.batches.push(Batch {
                tex,
                scissor: self.scissor,
                start: end - 6,
                end,
            }),
        }
    }

    /// A rounded box of `half` size centred on the origin of `xf`'s source
    /// space. A circle is a box whose radius is its half size.
    fn shape(&mut self, xf: &Affine, half: (f32, f32), radius: f32, color: Color) {
        if color.a == 0 || !(half.0 > 0.0 && half.1 > 0.0) {
            return;
        }
        let ppu = (xf.a * xf.d - xf.b * xf.c).abs().sqrt();
        if !ppu.is_finite() || ppu < 1e-6 {
            return;
        }
        let r = radius.clamp(0.0, half.0.min(half.1));
        // Grow the quad by 1.5 px so the anti-aliased edge has room.
        let m = 1.5 / ppu;
        let (hx, hy) = (half.0 + m, half.1 + m);
        let corner = |lx: f32, ly: f32| {
            let p = xf.apply(Vec2::new(lx, ly));
            Vertex {
                pos: [p.x, p.y],
                uv: [0.0, 0.0],
                local: [lx, ly],
                params: [half.0, half.1, r, ppu],
                kind: KIND_SHAPE,
                color: rgba(color),
            }
        };
        self.quad(
            TexId::White,
            [
                corner(-hx, -hy),
                corner(hx, -hy),
                corner(hx, hy),
                corner(-hx, hy),
            ],
        );
    }

    /// A picture of `size` centred on the origin of `xf`'s source space.
    fn picture(&mut self, xf: &Affine, tex: TexId, size: (f32, f32), tint: Color) {
        let (hw, hh) = (size.0 / 2.0, size.1 / 2.0);
        let corner = |lx: f32, ly: f32, u: f32, v: f32| {
            let p = xf.apply(Vec2::new(lx, ly));
            Vertex {
                pos: [p.x, p.y],
                uv: [u, v],
                local: [0.0, 0.0],
                params: [0.0; 4],
                kind: KIND_TEXTURE,
                color: rgba(tint),
            }
        };
        self.quad(
            tex,
            [
                corner(-hw, -hh, 0.0, 0.0),
                corner(hw, -hh, size.0, 0.0),
                corner(hw, hh, size.0, size.1),
                corner(-hw, hh, 0.0, size.1),
            ],
        );
    }

    /// A flat-coloured pixel rectangle.
    fn solid(&mut self, r: Rect, color: Color) {
        if color.a == 0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let v = |x: f32, y: f32| Vertex {
            pos: [x, y],
            uv: [0.0, 0.0],
            local: [0.0, 0.0],
            params: [0.0; 4],
            kind: KIND_SOLID,
            color: rgba(color),
        };
        self.quad(
            TexId::White,
            [
                v(r.x, r.y),
                v(r.right(), r.y),
                v(r.right(), r.bottom()),
                v(r.x, r.bottom()),
            ],
        );
    }

    /// A pixel rectangle with rounded corners.
    fn rounded(&mut self, r: Rect, radius: f32, color: Color) {
        let c = r.centre();
        self.shape(
            &Affine::translation(c),
            (r.w / 2.0, r.h / 2.0),
            radius,
            color,
        );
    }

    /// A line of `thickness` pixels with square ends, as a rotated shape.
    fn line(&mut self, a: Vec2, b: Vec2, thickness: f32, color: Color) {
        let d = b - a;
        let len = (d.x * d.x + d.y * d.y).sqrt();
        if len < 1e-3 {
            return;
        }
        let angle = d.y.atan2(d.x).to_degrees();
        let mid = Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let xf = Affine::translation(mid).then_after(&Affine::rotation_deg(angle));
        self.shape(&xf, (len / 2.0, thickness / 2.0), thickness / 2.0, color);
    }

    fn art(&mut self, xf: &Affine, art: &Art, images: &mut ImageStore) {
        match art {
            Art::Rect { w, h, color } => self.shape(xf, (w / 2.0, h / 2.0), 0.0, *color),
            Art::RoundRect {
                w,
                h,
                radius,
                color,
            } => {
                self.shape(xf, (w / 2.0, h / 2.0), *radius, *color);
            }
            Art::Circle { radius, color } => self.shape(xf, (*radius, *radius), *radius, *color),
            Art::Image(path) => match images.get(path) {
                Some(p) => self.picture(
                    xf,
                    TexId::Image(p.index),
                    (p.width as f32, p.height as f32),
                    Color::rgb(255, 255, 255),
                ),
                None => self.shape(xf, (MISSING_HALF, MISSING_HALF), 8.0, MISSING_COLOR),
            },
        }
    }

    /// Write `text` with its top-left at `(x, y)`; `px` must be whole.
    fn text(&mut self, engine: &mut TextEngine, x: f32, y: f32, text: &str, px: f32, color: Color) {
        let (placed, _) = engine.font.layout(text, px);
        for pc in placed {
            let Some(slot) = engine.cache.get(&engine.font, &mut engine.atlas, pc.ch, px) else {
                continue;
            };
            let gx = (x + pc.x + slot.left).round();
            let gy = (y + pc.baseline + slot.top).round();
            let (w, h) = (slot.rect.w as f32, slot.rect.h as f32);
            let (u0, v0) = (slot.rect.x as f32, slot.rect.y as f32);
            let v = |px_x: f32, px_y: f32, u: f32, vv: f32| Vertex {
                pos: [px_x, px_y],
                uv: [u, vv],
                local: [0.0, 0.0],
                params: [0.0; 4],
                kind: KIND_TEXTURE,
                color: rgba(color),
            };
            self.quad(
                TexId::Glyphs,
                [
                    v(gx, gy, u0, v0),
                    v(gx + w, gy, u0 + w, v0),
                    v(gx + w, gy + h, u0 + w, v0 + h),
                    v(gx, gy + h, u0, v0 + h),
                ],
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The frame
// ---------------------------------------------------------------------------

/// Colours of the speaker name: on screen, voice-over, off screen.
const NAME_COLORS: [Color; 3] = [
    Color::rgb(255, 214, 120),
    Color::rgb(140, 205, 255),
    Color::rgb(200, 205, 215),
];
// Blending happens in linear light on an sRGB target, which lifts dark colours:
// the box needs a high alpha to stay dark over a bright picture.
const SUBTITLE_BOX: Color = Color {
    r: 8,
    g: 10,
    b: 16,
    a: 228,
};
const SUBTITLE_TEXT: Color = Color::rgb(255, 255, 255);

impl FramePlan {
    /// Plan the frame for `state` on a target of `target` pixels.
    pub fn build(
        stage: Stage,
        state: &SceneState<'_>,
        target: (u32, u32),
        res: &mut Resources,
        hud: Option<&Hud>,
    ) -> FramePlan {
        let target = (target.0.max(1), target.1.max(1));
        let vp = fit_viewport(target, stage);
        let view = view_affine(&vp, stage, &state.camera);
        let mut p = Painter::new(target);

        // The picture, clipped to its area.
        p.set_scissor(scissor_of(&vp, target));
        for l in &state.back_layers {
            let xf = view.then_after(&Affine::translation(l.position));
            p.art(&xf, &l.layer.art, &mut res.images);
        }
        for c in &state.characters {
            for part in &c.parts {
                let xf = view.then_after(&part.world);
                p.art(&xf, part.part.mouth_art(c.mouth), &mut res.images);
            }
        }
        for l in &state.front_layers {
            let xf = view.then_after(&Affine::translation(l.position));
            p.art(&xf, &l.layer.art, &mut res.images);
        }
        if let Some(map) = &state.map_overlay {
            draw_map(&mut p, &stage_affine(&vp, stage), &vp, map, &mut res.text);
        }

        // Over the whole target.
        p.set_scissor(PixelRect {
            x: 0,
            y: 0,
            w: target.0,
            h: target.1,
        });
        if state.fade.alpha > 0.0 {
            p.solid(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: target.0 as f32,
                    h: target.1 as f32,
                },
                with_alpha(state.fade.color, state.fade.alpha),
            );
        }
        let subtitle = match &state.subtitle {
            Some(s) => {
                let layout = res.subtitle_layout(s.speaker_name, s.text, &vp);
                draw_subtitle(&mut p, s, &layout, &mut res.text);
                Some(layout)
            }
            None => None,
        };
        if let Some(hud) = hud {
            draw_hud(&mut p, hud, target, &mut res.text);
        }

        FramePlan {
            vertices: p.vertices,
            indices: p.indices,
            batches: p.batches,
            layout: FrameLayout {
                target,
                viewport: vp,
                subtitle,
            },
        }
    }
}

fn draw_subtitle(
    p: &mut Painter,
    s: &Subtitle<'_>,
    layout: &SubtitleLayout,
    text: &mut TextEngine,
) {
    p.rounded(layout.rect, layout.corner_radius, SUBTITLE_BOX);
    let name_color = match s.delivery {
        Delivery::OnScreen => NAME_COLORS[0],
        Delivery::VoiceOver => NAME_COLORS[1],
        Delivery::OffScreen => NAME_COLORS[2],
    };
    let px = layout.font_px;
    p.text(
        text,
        layout.name_origin.0,
        layout.name_origin.1,
        s.speaker_name,
        px,
        name_color,
    );
    for (line, (x, y)) in &layout.lines {
        p.text(text, *x, *y, line, px, SUBTITLE_TEXT);
    }
}

fn draw_map(
    p: &mut Painter,
    to_px: &Affine,
    vp: &Rect,
    map: &MapOverlayState<'_>,
    text: &mut TextEngine,
) {
    let k = to_px.a; // pixels per stage unit
    let px = (vp.h * 0.026).round().clamp(12.0, 48.0);
    let arrow = Color::rgb(200, 52, 40);
    let pin = Color::rgb(226, 72, 58);
    let ring = Color::rgb(255, 255, 255);
    let label_box = Color {
        r: 8,
        g: 10,
        b: 16,
        a: 210,
    };
    let thick = (8.0 * k).max(2.0);
    for m in map.markers {
        let at = to_px.apply(m.at);
        let anchor = match m.to {
            Some(to) => {
                let tip = to_px.apply(to);
                p.line(at, tip, thick, arrow);
                // Two short strokes make the head, swept back 28 degrees.
                let d = at - tip;
                let len = (d.x * d.x + d.y * d.y).sqrt();
                if len > 1e-3 {
                    let back = d.y.atan2(d.x);
                    for side in [-1.0_f32, 1.0] {
                        let a = back + side * 28.0_f32.to_radians();
                        let end = Vec2::new(tip.x + a.cos() * 40.0 * k, tip.y + a.sin() * 40.0 * k);
                        p.line(tip, end, thick, arrow);
                    }
                }
                at
            }
            None => {
                let c = Affine::translation(at);
                p.shape(&c, (17.0 * k, 17.0 * k), 17.0 * k, ring);
                p.shape(&c, (13.0 * k, 13.0 * k), 13.0 * k, pin);
                Vec2::new(at.x + 24.0 * k, at.y - px * 0.6)
            }
        };
        if !m.label.is_empty() {
            let w = text.width(&m.label, px);
            let pad = (px * 0.35).round();
            let (x, y) = if m.to.is_some() {
                (anchor.x - w / 2.0, anchor.y - px * 1.9)
            } else {
                (anchor.x, anchor.y)
            };
            let r = Rect {
                x: (x - pad).round(),
                y: (y - pad * 0.5).round(),
                w: (w + pad * 2.0).ceil(),
                h: (px * 1.3 + pad).ceil(),
            };
            p.rounded(r, pad, label_box);
            p.text(text, x, y, &m.label, px, ring);
        }
    }
}

const PANEL_BG: [Color; 4] = [
    Color {
        r: 10,
        g: 14,
        b: 24,
        a: 215,
    },
    Color {
        r: 10,
        g: 14,
        b: 24,
        a: 215,
    },
    Color {
        r: 44,
        g: 30,
        b: 6,
        a: 232,
    },
    Color {
        r: 58,
        g: 10,
        b: 12,
        a: 238,
    },
];
const PANEL_TEXT: [Color; 4] = [
    Color::rgb(255, 255, 255),
    Color::rgb(160, 176, 204),
    Color::rgb(255, 205, 100),
    Color::rgb(255, 160, 150),
];

fn tone_index(t: HudTone) -> usize {
    match t {
        HudTone::Normal => 0,
        HudTone::Dim => 1,
        HudTone::Warning => 2,
        HudTone::Error => 3,
    }
}

/// The font size of the HUD panels for a target of this height.
pub fn panel_font_px(target_height: f32) -> f32 {
    (target_height * 0.026).round().clamp(12.0, 36.0)
}

/// Stack the panels down the left edge, each as wide as its longest line (at
/// most the window), lines wrapped, and cut with a "... N more lines" when they
/// would run into the bottom `reserved` pixels.
fn draw_panels(
    p: &mut Painter,
    panels: &[HudPanel],
    target: (u32, u32),
    reserved: f32,
    text: &mut TextEngine,
) {
    let (w, h) = (target.0 as f32, target.1 as f32);
    let px = panel_font_px(h);
    let line_h = (px * 1.3).round();
    let pad = (px * 0.5).round();
    let margin = pad;
    let max_text_w = (w - 2.0 * margin - 2.0 * pad).max(px);
    let mut y = margin;
    for panel in panels {
        let mut rows: Vec<(String, HudTone)> = Vec::new();
        for line in &panel.lines {
            for row in text.wrap(&line.text, px, max_text_w) {
                rows.push((row, line.tone));
            }
        }
        if rows.is_empty() {
            continue;
        }
        let room = ((h - reserved - y - 2.0 * pad) / line_h).floor();
        if room < 1.0 {
            break;
        }
        let room = room as usize;
        if rows.len() > room {
            if room >= 2 {
                let hidden = rows.len() - (room - 1);
                rows.truncate(room - 1);
                rows.push((format!("... {hidden} more lines"), HudTone::Dim));
            } else {
                rows.truncate(1);
            }
        }
        let widest = rows
            .iter()
            .map(|(r, _)| text.width(r, px))
            .fold(0.0_f32, f32::max);
        let rect = Rect {
            x: margin,
            y,
            w: (widest + 2.0 * pad).ceil().min(w - 2.0 * margin),
            h: rows.len() as f32 * line_h + 2.0 * pad,
        };
        p.rounded(rect, pad * 0.6, PANEL_BG[tone_index(panel.tone)]);
        for (i, (row, tone)) in rows.iter().enumerate() {
            p.text(
                text,
                rect.x + pad,
                rect.y + pad + i as f32 * line_h,
                row,
                px,
                PANEL_TEXT[tone_index(*tone)],
            );
        }
        y += rect.h + margin * 0.5;
    }
}

fn draw_hud(p: &mut Painter, hud: &Hud, target: (u32, u32), text: &mut TextEngine) {
    let (w, h) = (target.0 as f32, target.1 as f32);
    let bar = (h * 0.008).round().max(4.0);
    p.solid(
        Rect {
            x: 0.0,
            y: h - bar,
            w,
            h: bar,
        },
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 150,
        },
    );
    p.solid(
        Rect {
            x: 0.0,
            y: h - bar,
            w: w * hud.progress.clamp(0.0, 1.0),
            h: bar,
        },
        Color::rgb(232, 176, 74),
    );
    for m in &hud.shot_marks {
        p.solid(
            Rect {
                x: (w * m.clamp(0.0, 1.0)).round().min(w - 2.0),
                y: h - bar * 2.0,
                w: 2.0,
                h: bar * 2.0,
            },
            Color::rgb(255, 255, 255),
        );
    }
    draw_panels(p, &hud.panels, target, bar * 2.0, text);
    if hud.paused {
        let s = (h * 0.045).round().max(12.0);
        let (x, y) = (w - s * 2.0, s * 0.8);
        let c = Color {
            r: 255,
            g: 255,
            b: 255,
            a: 220,
        };
        p.solid(
            Rect {
                x,
                y,
                w: s * 0.32,
                h: s,
            },
            c,
        );
        p.solid(
            Rect {
                x: x + s * 0.68,
                y,
                w: s * 0.32,
                h: s,
            },
            c,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::load_str;
    use crate::model::Scene;

    fn example() -> Scene {
        let text = include_str!("../../../../assets/scenes/example_bridge.ron");
        load_str(text, None).expect("example scene").scene
    }

    fn plan_at(scene: &Scene, t: i32, target: (u32, u32)) -> FramePlan {
        let state = scene.sample(t);
        let mut res = Resources::new("does-not-exist");
        FramePlan::build(scene.stage, &state, target, &mut res, None)
    }

    /// Colours of the quads in drawing order (one entry per quad).
    fn colors(plan: &FramePlan) -> Vec<[u8; 4]> {
        plan.vertices.chunks(4).map(|q| q[0].color).collect()
    }

    #[test]
    fn the_stage_is_fitted_and_centred() {
        let stage = Stage {
            width: 1920,
            height: 1080,
        };
        let full = fit_viewport((1920, 1080), stage);
        assert_eq!((full.x, full.y, full.w, full.h), (0.0, 0.0, 1920.0, 1080.0));
        let sq = fit_viewport((1000, 1000), stage);
        assert_eq!((sq.x, sq.w, sq.h), (0.0, 1000.0, 563.0));
        assert!((sq.y - 218.0).abs() <= 1.0);
        let wide = fit_viewport((2000, 1000), stage);
        assert_eq!(wide.h, 1000.0);
        assert!((wide.x - (2000.0 - wide.w) / 2.0).abs() <= 1.0);
    }

    #[test]
    fn the_camera_centre_lands_in_the_middle_and_zoom_scales() {
        let stage = Stage {
            width: 1920,
            height: 1080,
        };
        let vp = fit_viewport((1280, 720), stage);
        let cam = CameraState {
            center: Vec2::new(100.0, -40.0),
            zoom: 2.0,
            shake: Vec2::ZERO,
        };
        let v = view_affine(&vp, stage, &cam);
        let mid = v.apply(Vec2::new(100.0, -40.0));
        assert!((mid.x - 640.0).abs() < 1e-3 && (mid.y - 360.0).abs() < 1e-3);
        // One stage unit is 1280/1920 px at zoom 1, twice that at zoom 2.
        let one = v.apply(Vec2::new(101.0, -40.0)).x - mid.x;
        assert!((one - 2.0 * 1280.0 / 1920.0).abs() < 1e-4);
        // The shake moves everything.
        let shaken = view_affine(
            &vp,
            stage,
            &CameraState {
                shake: Vec2::new(10.0, 0.0),
                ..cam
            },
        );
        assert!(shaken.apply(Vec2::ZERO).x > v.apply(Vec2::ZERO).x);
    }

    #[test]
    fn layers_and_parts_are_drawn_back_to_front() {
        let scene = example();
        let plan = plan_at(&scene, 4000, (1280, 720));
        let c = colors(&plan);
        let rgb = |hex: &str| {
            let col = Color::parse(hex).unwrap();
            [col.r, col.g, col.b, col.a]
        };
        let at = |hex: &str| {
            c.iter()
                .position(|q| *q == rgb(hex))
                .unwrap_or_else(|| panic!("{hex} not drawn"))
        };
        // Wall (z -20) before floor (z -19) before the major's legs (z -1),
        // torso (0), arm (2), head (3), eye (4).
        let order = [
            "#6b7a99", "#40495b", "#223952", "#3a5f8a", "#2e4c6e", "#e8c39e", "#000000",
        ];
        let positions: Vec<usize> = order.iter().map(|h| at(h)).collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
        // The map table (shot "map") is not part of this shot.
        assert!(!c.contains(&rgb("#d9c9a0")));
    }

    #[test]
    fn the_map_shot_draws_the_table_in_front_and_the_markers_over_it() {
        let scene = example();
        let plan = plan_at(&scene, 10_000, (1280, 720));
        let c = colors(&plan);
        let table = Color::parse("#d9c9a0").unwrap();
        let t = c
            .iter()
            .position(|q| *q == [table.r, table.g, table.b, 255])
            .expect("map table drawn");
        let pin = c
            .iter()
            .position(|q| *q == [226, 72, 58, 255])
            .expect("pin");
        let arrow = c
            .iter()
            .position(|q| *q == [200, 52, 40, 255])
            .expect("arrow");
        assert!(t < pin && t < arrow, "markers are over the table");
    }

    #[test]
    fn the_scene_is_clipped_to_the_picture_and_the_fade_covers_the_target() {
        let scene = example();
        // 16:10 target: bars above and below.
        let plan = plan_at(&scene, 31_200, (1000, 800));
        let vp = plan.layout.viewport;
        assert!(vp.y > 0.0 && vp.x == 0.0);
        let first = plan.batches.first().unwrap();
        assert_eq!(first.scissor.y, vp.y as u32);
        assert_eq!(first.scissor.h, vp.h as u32);
        let last = plan.batches.last().unwrap();
        assert_eq!(
            last.scissor,
            PixelRect {
                x: 0,
                y: 0,
                w: 1000,
                h: 800
            }
        );
        // The fade: FadeOut 30800..31600, so at 31200 it is half way to black.
        let fade = plan
            .vertices
            .chunks(4)
            .rev()
            .find(|q| q[0].kind == KIND_SOLID)
            .expect("fade quad");
        assert!(
            (i32::from(fade[0].color[3]) - 128).abs() <= 2,
            "{:?}",
            fade[0].color
        );
        assert_eq!(fade[0].pos, [0.0, 0.0]);
        assert_eq!(fade[2].pos, [1000.0, 800.0]);
        assert!(
            plan.layout.subtitle.is_none(),
            "no line is up during the fade"
        );
    }

    #[test]
    fn a_line_in_progress_gets_a_box_and_text() {
        let scene = example();
        let mut res = Resources::new("x");
        let state = scene.sample(4000);
        let plan = FramePlan::build(scene.stage, &state, (1280, 720), &mut res, None);
        let sub = plan.layout.subtitle.expect("subtitle at 4000");
        assert_eq!(sub.name, "Major Halversen");
        assert!(sub.font_px >= 22.0);
        let glyphs = plan
            .batches
            .iter()
            .filter(|b| b.tex == TexId::Glyphs)
            .count();
        assert!(glyphs >= 1);
        let glyph_quads: usize = plan
            .batches
            .iter()
            .filter(|b| b.tex == TexId::Glyphs)
            .map(|b| (b.end - b.start) as usize / 6)
            .sum();
        let letters = "Major Halversen"
            .chars()
            .chain(sub.lines.iter().flat_map(|(l, _)| l.chars()))
            .filter(|c| !c.is_whitespace())
            .count();
        assert_eq!(glyph_quads, letters, "one quad per visible letter");
    }

    #[test]
    fn quads_outside_the_picture_are_not_emitted() {
        let scene = example();
        let mut state = scene.sample(1500);
        let full = FramePlan::build(
            scene.stage,
            &state,
            (1280, 720),
            &mut Resources::new("x"),
            None,
        )
        .vertices
        .len();
        state.camera.center = Vec2::new(50_000.0, 0.0);
        state.camera.zoom = 8.0;
        let away = FramePlan::build(
            scene.stage,
            &state,
            (1280, 720),
            &mut Resources::new("x"),
            None,
        );
        assert!(
            away.vertices.is_empty(),
            "{} vs {full}",
            away.vertices.len()
        );
    }

    #[test]
    fn a_missing_picture_is_a_visible_placeholder_not_a_hole() {
        let scene = load_str(
            r##"Scene(name: "m", layers: [(name: "l", art: Image("art/none.png"))])"##,
            None,
        )
        .unwrap()
        .scene;
        let mut res = Resources::new("does-not-exist");
        let plan = FramePlan::build(scene.stage, &scene.sample(0), (640, 360), &mut res, None);
        assert_eq!(plan.vertices.len(), 4);
        assert_eq!(plan.vertices[0].color, [255, 0, 255, 160]);
        assert_eq!(res.images.failures().len(), 1);
    }

    #[test]
    fn a_loaded_picture_is_a_textured_quad_of_its_own_size() {
        let scene = load_str(
            r##"Scene(name: "m", layers: [(name: "l", art: Image("art/p.png"))])"##,
            None,
        )
        .unwrap()
        .scene;
        let mut res = Resources::new("x");
        res.images.insert(
            "art/p.png",
            super::super::images::Picture {
                width: 192,
                height: 108,
                pixels: vec![255; 192 * 108 * 4],
            },
        );
        let plan = FramePlan::build(scene.stage, &scene.sample(0), (1920, 1080), &mut res, None);
        assert_eq!(plan.batches[0].tex, TexId::Image(0));
        let q = &plan.vertices[0..4];
        assert_eq!(q[0].kind, KIND_TEXTURE);
        assert_eq!(q[2].uv, [192.0, 108.0]);
        assert_eq!(
            (q[2].pos[0] - q[0].pos[0], q[2].pos[1] - q[0].pos[1]),
            (192.0, 108.0)
        );
    }

    #[test]
    fn rounded_rectangles_and_circles_carry_their_radius() {
        let scene = load_str(
            r##"Scene(name: "s", layers: [
                (name: "a", art: RoundRect(w: 100, h: 60, radius: 500, color: "#ffffff"), z: -2),
                (name: "b", art: Circle(radius: 25, color: "#ff0000"), z: -1),
            ])"##,
            None,
        )
        .unwrap()
        .scene;
        let plan = plan_at(&scene, 0, (1920, 1080));
        let a = &plan.vertices[0];
        assert_eq!(
            &a.params[..3],
            &[50.0, 30.0, 30.0],
            "radius capped at the short half-side"
        );
        let b = &plan.vertices[4];
        assert_eq!(&b.params[..3], &[25.0, 25.0, 25.0]);
    }

    #[test]
    fn the_hud_draws_a_progress_bar_without_any_text() {
        let scene = example();
        let state = scene.sample(4000);
        let hud = Hud {
            progress: 0.5,
            shot_marks: vec![0.0, 0.2, 0.5],
            paused: true,
            panels: Vec::new(),
        };
        let mut res = Resources::new("x");
        let with = FramePlan::build(scene.stage, &state, (1280, 720), &mut res, Some(&hud));
        let without = FramePlan::build(scene.stage, &state, (1280, 720), &mut res, None);
        // Track + fill + 3 marks + 2 pause bars.
        assert_eq!(with.vertices.len() - without.vertices.len(), 7 * 4);
        let start = without.vertices.len();
        let fill = &with.vertices[start + 4..start + 8];
        assert_eq!(fill[0].color, [232, 176, 74, 255]);
        assert_eq!(fill[1].pos[0], 640.0);
    }
    fn plan_with_panels(panels: Vec<HudPanel>, target: (u32, u32)) -> (FramePlan, FramePlan) {
        let scene = example();
        let state = scene.sample(1000);
        let mut res = Resources::new("x");
        let base = Hud::default();
        let with = Hud {
            panels,
            ..Hud::default()
        };
        (
            FramePlan::build(scene.stage, &state, target, &mut res, Some(&with)),
            FramePlan::build(scene.stage, &state, target, &mut res, Some(&base)),
        )
    }

    #[test]
    fn a_panel_adds_a_box_and_text_inside_the_window() {
        let panel = HudPanel {
            tone: HudTone::Error,
            lines: vec![
                HudLine::new("scene.ron:12:5: unknown field `colour`", HudTone::Error),
                HudLine::new("keeping the last good version", HudTone::Dim),
            ],
        };
        let (with, without) = plan_with_panels(vec![panel], (1280, 720));
        assert!(with.vertices.len() > without.vertices.len() + 4 * 20);
        for v in &with.vertices[without.vertices.len()..] {
            assert!(v.pos[0] >= 0.0 && v.pos[0] <= 1280.0, "{:?}", v.pos);
            assert!(v.pos[1] >= 0.0 && v.pos[1] <= 720.0, "{:?}", v.pos);
        }
        // The first quad after the bar is the box, in the error colour.
        let first = &with.vertices[without.vertices.len()];
        assert_eq!(first.color, [58, 10, 12, 238]);
    }

    #[test]
    fn a_long_line_wraps_and_a_tall_panel_is_cut_to_the_window() {
        let long = "x".repeat(400);
        let lines = (0..200)
            .map(|i| HudLine::new(format!("{i} {long}"), HudTone::Warning))
            .collect();
        let panel = HudPanel {
            tone: HudTone::Warning,
            lines,
        };
        let (with, without) = plan_with_panels(vec![panel], (800, 450));
        let bar = (450.0_f32 * 0.008).round().max(4.0);
        for v in &with.vertices[without.vertices.len()..] {
            assert!(v.pos[0] >= -1.0 && v.pos[0] <= 801.0, "{:?}", v.pos);
            assert!(
                v.pos[1] <= 450.0 - bar * 2.0 + 1.5,
                "stays above the progress bar: {:?}",
                v.pos
            );
        }
    }

    #[test]
    fn panels_stack_downwards_and_an_empty_one_is_skipped() {
        let one = |t: &str| HudPanel {
            tone: HudTone::Normal,
            lines: vec![HudLine::new(t, HudTone::Normal)],
        };
        let empty = HudPanel {
            tone: HudTone::Normal,
            lines: vec![],
        };
        let (with, without) = plan_with_panels(vec![one("a"), empty, one("b")], (1280, 720));
        let boxes: Vec<f32> = with.vertices[without.vertices.len()..]
            .chunks(4)
            .filter(|q| q[0].color == [10, 14, 24, 215] && q[0].kind == super::KIND_SHAPE)
            .map(|q| q[0].pos[1])
            .collect();
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        assert!(boxes[1] > boxes[0]);
    }
}
