//! The minimap: a baked top-down terrain image, unit dots, the camera's
//! field of view, and click/drag to move the camera.
//!
//! # Rendering to a texture
//! The terrain image is *baked on the CPU* by [`Minimap::bake`], one gameplay
//! cell at a time (WP2.3). A cell is drawn in the flat minimap colour of its
//! terrain type (water, rock, road, ore, bridge, grass), so the minimap shows
//! what the game rules see. The colour comes from the type's 3D look
//! (`assets/config/terrain_look.ron`, [`sf_render::terrain_look::TerrainLook::minimap_color`]): the
//! look is the one source of a type's colour, so the minimap and the main view
//! agree. A map without a terrain layer falls back
//! to the average colour of the cell's procedural material. The relief is
//! shaded lightly on top (light from the north-west, one shade per cell,
//! at most [`RELIEF_SHADE`] either way) so hills read without hiding the type.
//!
//! The image has a whole number of pixels per cell (nearest-neighbour), so
//! cells stay crisp under the UI's bilinear sampler: a 64-cell map becomes
//! 256 x 256 (4 px per cell), a 256-cell map 256 x 256 (1 px per cell, shown
//! slightly smaller than 1 px per cell in a 200 px panel, where a one-cell
//! road is still a visible line). A map larger than the requested side is
//! sampled down. Baking is a few milliseconds for 256 x 256 cells and happens
//! once, when the map is set; the result is uploaded as a texture by
//! `UiRenderer::set_minimap`. The terrain is static, so there is no update
//! rate and the per-frame cost is the same one textured quad as before: call
//! [`Minimap::bake`] and `set_minimap` again when the terrain changes (terrain
//! edits, later). A GPU top-down render of the real terrain pass was rejected:
//! it would tie the minimap to the terrain chunks and LOD of WP1.8 and cost a
//! depth buffer and a second scene pass for an image that never changes.
//!
//! Everything dynamic is drawn on top as UI quads every frame: unit dots
//! (a few pixels each) and the field-of-view frame.
//!
//! # Coordinates
//! The map is `map_w x map_h` metres with `x` east and `z` south; north is
//! up on the minimap, like on screen. The map is centred in the panel with
//! its aspect ratio kept ([`MinimapGeometry`]).
//!
//! A map side that is not a multiple of the 4 m cell ends in a partial cell:
//! the image holds whole cells (`Map::size_in_cells` rounds up), so it covers
//! a little more than the map. The panel shows the map in metres and only the
//! part of the image that lies on it ([`Minimap::uv`]), so every cell is drawn
//! where its metres are and the dots, the field of view and the clicks agree.

use sf_data::map::Map;
use sf_data::terrain::TerrainTypes;
use sf_math::CELL_METRES;
use sf_render::Camera;
use sf_render::camera::DVec2;
use sf_render::terrain_look::TerrainLooks;

use crate::color::Color;
use crate::draw::Texture;
use crate::layout::Rect;
use crate::ui::Ui;

/// How far the relief shading may brighten or darken a cell's colour (a
/// fraction: 0.18 = at most 18 % either way). Small on purpose: the colour
/// tells the terrain type, the shade only hints at the hills.
pub const RELIEF_SHADE: f32 = 0.18;
/// Brightness change per metre of height difference across a cell, measured
/// from the north-west to the south-east side of its surroundings. A hill of
/// 6 m over 40 m (the demo map) reaches the limit on its flanks.
const RELIEF_GAIN: f32 = 0.2;
/// Colour of a cell whose terrain type or material is unknown.
const UNKNOWN: [u8; 3] = [128, 128, 128];

/// The baked image: sRGB RGBA8, row-major, top row = north.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinimapImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A baked minimap of one map.
#[derive(Clone, Debug)]
pub struct Minimap {
    image: MinimapImage,
    map_size: (f32, f32),
    uv: [f32; 4],
}

impl Minimap {
    /// Bake `map` into an image whose longer side is at most `max_side`
    /// pixels, with the shipped terrain types ([`TerrainTypes::embedded`])
    /// and looks ([`TerrainLooks::embedded`]).
    pub fn bake(map: &Map, max_side: u32) -> Self {
        Self::bake_with(
            map,
            max_side,
            &TerrainTypes::embedded(),
            &TerrainLooks::embedded(),
        )
    }

    /// [`Minimap::bake`] with an explicit table of terrain types and looks.
    pub fn bake_with(map: &Map, max_side: u32, types: &TerrainTypes, looks: &TerrainLooks) -> Self {
        let image = bake_terrain_with(map, max_side, types, looks);
        let (cw, ch) = map.size_in_cells();
        let cell = CELL_METRES as f32;
        // The image spans whole cells; the map ends inside the last one.
        let uv = [
            0.0,
            0.0,
            image.width as f32 * map.width() as f32 / (cw.max(1) as f32 * cell),
            image.height as f32 * map.height() as f32 / (ch.max(1) as f32 * cell),
        ];
        Self {
            image,
            map_size: (map.width() as f32, map.height() as f32),
            uv,
        }
    }

    pub fn image(&self) -> &MinimapImage {
        &self.image
    }

    /// The part of the image that lies on the map, in image pixels
    /// (`[u0, v0, u1, v1]`, texels): the whole image when the map is a whole number of
    /// cells, less when it ends in a partial cell.
    pub fn uv(&self) -> [f32; 4] {
        self.uv
    }

    pub fn map_size(&self) -> (f32, f32) {
        self.map_size
    }
}

/// Average sRGB color of each material of the map (the colour of a cell on a
/// map without a terrain layer).
fn material_colors(map: &Map) -> Vec<[u8; 3]> {
    map.materials()
        .iter()
        .map(|name| {
            let tex = sf_render::terrain::material_texture(name);
            let n = (tex.len() / 4).max(1) as u64;
            let mut sum = [0u64; 3];
            for px in tex.chunks_exact(4) {
                for (s, c) in sum.iter_mut().zip(px) {
                    *s += u64::from(*c);
                }
            }
            [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
        })
        .collect()
}

/// The shade factor of gameplay cell `(cx, cz)`: height at the north-west of
/// its surroundings minus the south-east, so a slope that faces the light is
/// brighter. 1.0 on flat ground.
fn relief_shade(map: &Map, cx: i64, cz: i64) -> f32 {
    let cell = i64::from(CELL_METRES);
    let (mx, mz) = (cx * cell + cell / 2, cz * cell + cell / 2);
    let reach = cell;
    let slope =
        map.vertex_height(mx - reach, mz - reach) - map.vertex_height(mx + reach, mz + reach);
    (1.0 + slope * RELIEF_GAIN).clamp(1.0 - RELIEF_SHADE, 1.0 + RELIEF_SHADE)
}

/// The colour of every gameplay cell (row-major, `cw * ch`), lightly shaded by
/// the relief: the minimap colour of the cell's terrain type (from its look),
/// or the average of its material when the map has no terrain layer.
fn cell_colors(map: &Map, types: &TerrainTypes, looks: &TerrainLooks) -> Vec<[u8; 3]> {
    let (cw, ch) = map.size_in_cells();
    // The procedural material textures are only generated for the fallback.
    let materials = if map.terrain().is_some() {
        Vec::new()
    } else {
        material_colors(map)
    };
    // The colour of every terrain id, looked up once (not per cell).
    let mut by_id = [UNKNOWN; 256];
    for t in types.iter() {
        by_id[usize::from(t.id)] = looks.look_for(&t.name).minimap_color();
    }
    let cell = i64::from(CELL_METRES);
    let mut out = Vec::with_capacity(cw as usize * ch as usize);
    for cz in 0..i64::from(ch) {
        for cx in 0..i64::from(cw) {
            let flat = match map.terrain() {
                Some(layer) => layer
                    .get(cx as u32, cz as u32)
                    .map_or(UNKNOWN, |id| by_id[usize::from(id)]),
                None => materials
                    .get(usize::from(
                        map.material_at(cx * cell + cell / 2, cz * cell + cell / 2),
                    ))
                    .copied()
                    .unwrap_or(UNKNOWN),
            };
            let shade = relief_shade(map, cx, cz);
            out.push(flat.map(|c| (f32::from(c) * shade).clamp(0.0, 255.0).round() as u8));
        }
    }
    out
}

/// See [`Minimap::bake`].
pub fn bake_terrain(map: &Map, max_side: u32) -> MinimapImage {
    bake_terrain_with(
        map,
        max_side,
        &TerrainTypes::embedded(),
        &TerrainLooks::embedded(),
    )
}

/// See [`Minimap::bake_with`].
pub fn bake_terrain_with(
    map: &Map,
    max_side: u32,
    types: &TerrainTypes,
    looks: &TerrainLooks,
) -> MinimapImage {
    let (cw, ch) = map.size_in_cells();
    let (cw, ch) = (cw.max(1), ch.max(1));
    let longest = cw.max(ch);
    let max_side = max_side.max(1);
    // A whole number of pixels per cell when the map fits (crisp cells under
    // bilinear sampling); sampled down when it does not.
    let (w, h) = if longest <= max_side {
        let s = max_side / longest;
        (cw * s, ch * s)
    } else {
        let k = f64::from(max_side) / f64::from(longest);
        (
            ((f64::from(cw) * k).round() as u32).max(1),
            ((f64::from(ch) * k).round() as u32).max(1),
        )
    };
    let colors = cell_colors(map, types, looks);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for py in 0..h {
        // The cell this row of pixels samples (nearest, centre of the pixel).
        let cz = ((u64::from(py) * 2 + 1) * u64::from(ch) / (u64::from(h) * 2)) as usize;
        for px in 0..w {
            let cx = ((u64::from(px) * 2 + 1) * u64::from(cw) / (u64::from(w) * 2)) as usize;
            let c = colors
                .get(cz * cw as usize + cx)
                .copied()
                .unwrap_or(UNKNOWN);
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    MinimapImage {
        width: w,
        height: h,
        rgba,
    }
}

/// The map-to-panel mapping: the map centred in `frame`, aspect kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinimapGeometry {
    content: Rect,
    map: (f32, f32),
}

impl MinimapGeometry {
    pub fn new(frame: Rect, map: (f32, f32)) -> Self {
        let (mw, mh) = (map.0.max(1e-3), map.1.max(1e-3));
        let k = (frame.w / mw).min(frame.h / mh).max(0.0);
        let (w, h) = (mw * k, mh * k);
        let content = Rect::new(
            frame.x + (frame.w - w) * 0.5,
            frame.y + (frame.h - h) * 0.5,
            w,
            h,
        );
        Self {
            content,
            map: (mw, mh),
        }
    }

    /// Where the map is drawn.
    pub fn content(&self) -> Rect {
        self.content
    }

    /// Screen position of a world position (`x` east, `z` south, metres).
    pub fn world_to_screen(&self, x: f32, z: f32) -> (f32, f32) {
        (
            self.content.x + x / self.map.0 * self.content.w,
            self.content.y + z / self.map.1 * self.content.h,
        )
    }

    /// World position under a screen position (not clamped).
    pub fn screen_to_world(&self, px: f32, py: f32) -> (f32, f32) {
        (
            (px - self.content.x) / self.content.w.max(1e-3) * self.map.0,
            (py - self.content.y) / self.content.h.max(1e-3) * self.map.1,
        )
    }

    /// Like [`MinimapGeometry::screen_to_world`], clamped to the map.
    pub fn screen_to_world_clamped(&self, px: f32, py: f32) -> (f32, f32) {
        let (x, z) = self.screen_to_world(px, py);
        (x.clamp(0.0, self.map.0), z.clamp(0.0, self.map.1))
    }
}

/// The four ground points under the corners of the screen, in world metres,
/// clockwise from the top-left, **not** limited to the map: they can lie
/// outside it when the view reaches past an edge. [`fov_polygon`] is what the
/// minimap draws.
///
/// `viewport` is the window size in pixels (the size the camera was given).
/// A corner whose ray misses the ground (at or above the horizon, only
/// possible with a very flat camera) is pulled toward the screen centre until
/// it hits, so the frame stays a finite quadrilateral.
pub fn fov_quad(camera: &Camera, viewport: (f32, f32)) -> [(f32, f32); 4] {
    let (w, h) = (f64::from(viewport.0), f64::from(viewport.1));
    let centre = DVec2::new(w * 0.5, h * 0.5);
    let corners = [
        DVec2::new(0.0, 0.0),
        DVec2::new(w, 0.0),
        DVec2::new(w, h),
        DVec2::new(0.0, h),
    ];
    corners.map(|c| {
        let p = camera.ground_point(c).unwrap_or_else(|| {
            // Bisect along corner -> centre for the farthest visible point.
            let (mut miss, mut hit) = (0.0f64, 1.0f64);
            for _ in 0..32 {
                let t = (miss + hit) * 0.5;
                if camera.ground_point(c + (centre - c) * t).is_some() {
                    hit = t;
                } else {
                    miss = t;
                }
            }
            camera
                .ground_point(c + (centre - c) * hit)
                .unwrap_or_else(|| camera.focus())
        });
        (p.x as f32, p.y as f32)
    })
}

/// The camera's field of view on the minimap: the part of the ground
/// quadrilateral ([`fov_quad`]) that lies on the map, clockwise from the
/// top-left, in world metres.
///
/// The quadrilateral is clipped against the map rectangle as a polygon
/// (Sutherland-Hodgman, [`clip_to_rect`]), so a view that reaches past an edge
/// is cut straight along that edge and keeps its true shape on the map, and
/// one that reaches past a corner gets the corner of the map as a vertex. The
/// result has 3 to 8 points; it is empty when the view does not touch the map
/// at all (the minimap then draws no frame).
pub fn fov_polygon(camera: &Camera, viewport: (f32, f32), map: (f32, f32)) -> Vec<(f32, f32)> {
    clip_to_rect(&fov_quad(camera, viewport), map)
}

/// Clip a polygon against the rectangle `(0, 0)` to `size` (Sutherland-Hodgman,
/// one rectangle edge after the other). The order of the points is kept; a
/// polygon wholly inside comes back unchanged and one wholly outside comes
/// back empty. Intersections are computed in `f64` and lie exactly on the
/// rectangle's edge.
pub fn clip_to_rect(polygon: &[(f32, f32)], size: (f32, f32)) -> Vec<(f32, f32)> {
    type P = (f64, f64);
    // Keep the points with `coordinate(p) * sign <= limit * sign`.
    fn clip_edge(input: &[P], axis: usize, limit: f64, keep_below: bool) -> Vec<P> {
        let value = |p: P| if axis == 0 { p.0 } else { p.1 };
        let inside = |p: P| {
            if keep_below {
                value(p) <= limit
            } else {
                value(p) >= limit
            }
        };
        let cut = |a: P, b: P| -> P {
            let t = (limit - value(a)) / (value(b) - value(a));
            if axis == 0 {
                (limit, a.1 + t * (b.1 - a.1))
            } else {
                (a.0 + t * (b.0 - a.0), limit)
            }
        };
        let mut out = Vec::with_capacity(input.len() + 2);
        for (i, &cur) in input.iter().enumerate() {
            let prev = input[(i + input.len() - 1) % input.len()];
            match (inside(prev), inside(cur)) {
                (true, true) => out.push(cur),
                (false, true) => {
                    out.push(cut(prev, cur));
                    out.push(cur);
                }
                (true, false) => out.push(cut(prev, cur)),
                (false, false) => {}
            }
        }
        out
    }
    let mut poly: Vec<P> = polygon
        .iter()
        .map(|&(x, z)| (f64::from(x), f64::from(z)))
        .collect();
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    for (axis, limit, keep_below) in [(0, 0.0, false), (0, w, true), (1, 0.0, false), (1, h, true)]
    {
        if poly.is_empty() {
            break;
        }
        poly = clip_edge(&poly, axis, limit, keep_below);
    }
    // A corner of the polygon on the edge, or a polygon touching the map
    // only along an edge or at a point, leaves repeated points: drop them.
    poly.dedup();
    while poly.len() > 1 && poly.first() == poly.last() {
        poly.pop();
    }
    if poly.len() < 3 {
        return Vec::new();
    }
    poly.into_iter()
        .map(|(x, z)| (x as f32, z as f32))
        .collect()
}

impl Ui {
    /// Draw the minimap in `frame` (physical pixels) and handle its input.
    ///
    /// * `fov`: the camera's field of view on the map from [`fov_polygon`]
    ///   (3 or more points; fewer draws nothing).
    /// * `units`: world position `(x, z)` and color of each dot.
    ///
    /// Returns the world point to move the camera to while the left button is
    /// pressed or dragged on the minimap (clamped to the map), else `None`.
    pub fn minimap(
        &mut self,
        minimap: &Minimap,
        id: &str,
        frame: Rect,
        fov: Option<&[(f32, f32)]>,
        units: impl IntoIterator<Item = ((f32, f32), Color)>,
    ) -> Option<(f32, f32)> {
        let resp = self.area(id, frame);
        let border = (2.0 * self.scale()).round().max(1.0);
        self.fill(frame, self.cfg.minimap_frame);
        let geom = MinimapGeometry::new(frame.inset(border), minimap.map_size);
        let content = geom.content();
        self.image(Texture::Minimap, content, minimap.uv(), Color::WHITE);

        let dot = self.scale().round().max(1.0) * 2.0;
        for ((x, z), color) in units {
            let (sx, sy) = geom.world_to_screen(x, z);
            if content.contains(sx, sy) {
                self.fill(
                    Rect::new((sx - dot * 0.5).round(), (sy - dot * 0.5).round(), dot, dot),
                    color,
                );
            }
        }

        if let Some(q) = fov.filter(|q| q.len() >= 3) {
            let pts: Vec<(f32, f32)> = q.iter().map(|&(x, z)| geom.world_to_screen(x, z)).collect();
            let thin = self.scale().round().max(1.0);
            let white = self.skin.white;
            let shadow = Color(0, 0, 0, 160);
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                self.list.line(white, a, b, thin + 2.0, shadow);
            }
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                self.list.line(white, a, b, thin, self.cfg.fov_frame);
            }
        }

        if resp.pressed || resp.dragging {
            let (cx, cy) = resp.cursor?;
            return Some(geom.screen_to_world_clamped(cx, cy));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{DVec3, DVec4};
    use sf_data::map::generate::{generate, generate_cells};
    use sf_render::CameraConfig;

    use crate::color::UiConfig;
    use crate::draw::Texture as Tex;
    use crate::ui::UiInput;

    /// The minimap colour of a terrain type id, from the shipped looks.
    fn mini(id: u8) -> [u8; 3] {
        let types = TerrainTypes::embedded();
        TerrainLooks::embedded()
            .look_for(&types.get(id).unwrap().name)
            .minimap_color()
    }

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn world_screen_round_trip_square_and_letterboxed() {
        for (frame, map) in [
            (Rect::new(10.0, 20.0, 200.0, 200.0), (256.0, 256.0)),
            (Rect::new(0.0, 0.0, 300.0, 150.0), (128.0, 128.0)),
            (Rect::new(5.0, 5.0, 150.0, 300.0), (400.0, 100.0)),
        ] {
            let g = MinimapGeometry::new(frame, map);
            for i in 0..=10 {
                for j in 0..=10 {
                    let (x, z) = (map.0 * i as f32 / 10.0, map.1 * j as f32 / 10.0);
                    let (sx, sy) = g.world_to_screen(x, z);
                    let (rx, rz) = g.screen_to_world(sx, sy);
                    assert!(
                        close(rx, x, 1e-3) && close(rz, z, 1e-3),
                        "{x},{z} -> {rx},{rz}"
                    );
                }
            }
            let c = g.content();
            assert!(c.x >= frame.x - 1e-3 && c.right() <= frame.right() + 1e-3);
            assert!(c.y >= frame.y - 1e-3 && c.bottom() <= frame.bottom() + 1e-3);
            let a = map.0 / map.1;
            assert!(close(c.w / c.h, a, a * 1e-4), "aspect kept");
        }
    }

    #[test]
    fn map_corners_land_on_content_corners_north_up() {
        let g = MinimapGeometry::new(Rect::new(100.0, 100.0, 200.0, 200.0), (256.0, 256.0));
        assert_eq!(
            g.world_to_screen(0.0, 0.0),
            (100.0, 100.0),
            "north-west is top-left"
        );
        assert_eq!(g.world_to_screen(256.0, 256.0), (300.0, 300.0));
        let (_, north) = g.world_to_screen(128.0, 10.0);
        let (_, south) = g.world_to_screen(128.0, 200.0);
        assert!(north < south, "smaller z = further up");
    }

    #[test]
    fn clicks_outside_the_content_clamp_to_the_map() {
        let g = MinimapGeometry::new(Rect::new(0.0, 0.0, 100.0, 100.0), (200.0, 100.0));
        assert_eq!(g.screen_to_world_clamped(-50.0, -50.0), (0.0, 0.0));
        assert_eq!(g.screen_to_world_clamped(500.0, 500.0), (200.0, 100.0));
        let (x, z) = g.screen_to_world_clamped(50.0, 50.0);
        assert!(
            close(x, 100.0, 1e-3) && close(z, 50.0, 1e-3),
            "centre of the letterboxed content"
        );
    }

    fn camera(map: f64, zoom: f64, focus: (f64, f64), size: (u32, u32)) -> Camera {
        let cfg = CameraConfig {
            map_size: DVec2::new(map, map),
            margin: 1000.0,
            ..CameraConfig::default()
        };
        let mut cam = Camera::new(cfg, size.0, size.1);
        cam.set_zoom(zoom);
        cam.set_focus(DVec2::new(focus.0, focus.1));
        cam
    }

    /// NDC of a ground point `(x, 0, z)` through the camera's f64 matrix.
    fn project(cam: &Camera, p: (f32, f32)) -> (f64, f64) {
        let m = cam.view_proj_f64();
        let clip = m * DVec4::new(f64::from(p.0), 0.0, f64::from(p.1), 1.0);
        let v = DVec3::new(clip.x, clip.y, clip.z) / clip.w;
        (v.x, v.y)
    }

    #[test]
    fn fov_quad_corners_are_the_projected_screen_corners() {
        // A big map and a generous margin so nothing is clamped.
        for (zoom, focus, size) in [
            (0.0, (20000.0, 20000.0), (1280, 720)),
            (0.3, (18000.0, 21000.0), (1280, 720)),
            (0.6, (20000.0, 19000.0), (800, 800)),
            (0.9, (21000.0, 20000.0), (1920, 1080)),
        ] {
            let cam = camera(40000.0, zoom, focus, size);
            let quad = fov_quad(&cam, (size.0 as f32, size.1 as f32));
            let expect = [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)]; // TL TR BR BL in NDC
            for (corner, want) in quad.iter().zip(expect) {
                let (nx, ny) = project(&cam, *corner);
                assert!(
                    (nx - want.0).abs() < 1e-3 && (ny - want.1).abs() < 1e-3,
                    "zoom {zoom}: {corner:?} projects to ({nx}, {ny}), want {want:?}"
                );
            }
            // And they are what the camera's own picking says.
            let direct = cam.ground_point(DVec2::new(0.0, 0.0)).unwrap();
            assert!(
                close(quad[0].0, direct.x as f32, 1e-2) && close(quad[0].1, direct.y as f32, 1e-2)
            );
        }
    }

    #[test]
    fn fov_quad_is_a_trapezoid_wider_at_the_far_edge_and_follows_the_focus() {
        let cam = camera(40000.0, 0.2, (20000.0, 20000.0), (1280, 720));
        let q = fov_quad(&cam, (1280.0, 720.0));
        assert!(
            q[0].1 < q[3].1,
            "top corners are further north than the bottom ones"
        );
        assert!(
            q[1].0 - q[0].0 > q[2].0 - q[3].0,
            "the far edge is wider (perspective)"
        );
        let panned = camera(40000.0, 0.2, (21000.0, 20000.0), (1280, 720));
        let q2 = fov_quad(&panned, (1280.0, 720.0));
        assert!(close(q2[0].0 - q[0].0, 1000.0, 0.1));
        assert!(close(q2[0].1, q[0].1, 0.1));
    }

    /// With the terrain as the camera's ground (WP1.8), the corners of the
    /// frame are the terrain points under the screen corners: they project
    /// back to the corners at the terrain's height, and they differ from the
    /// y = 0 plane's.
    #[test]
    fn fov_quad_on_relief_is_the_terrain_under_the_screen_corners() {
        use sf_render::{Ground, TerrainGround};
        let map = generate(256, 256, 1);
        let ground = TerrainGround::new(&map);
        let size = (1280_u32, 720_u32);
        let mut cam = camera(256.0, 0.45, (128.0, 128.0), size);
        cam.set_ground(std::sync::Arc::new(TerrainGround::new(&map)));
        let q = fov_quad(&cam, (size.0 as f32, size.1 as f32));
        let m = cam.view_proj_f64();
        let want = [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
        for (corner, want) in q.iter().zip(want) {
            let (x, z) = (f64::from(corner.0), f64::from(corner.1));
            let clip = m * DVec4::new(x, ground.height(x, z), z, 1.0);
            let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
            assert!(
                (nx - want.0).abs() < 1e-2 && (ny - want.1).abs() < 1e-2,
                "{corner:?} projects to ({nx}, {ny}), want {want:?}"
            );
        }
        let flat = camera(256.0, 0.45, (128.0, 128.0), size);
        let qf = fov_quad(&flat, (size.0 as f32, size.1 as f32));
        let apart = q
            .iter()
            .zip(&qf)
            .map(|(a, b)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt())
            .fold(0.0_f32, f32::max);
        println!("largest corner difference to the flat-plane frame: {apart:.2} m");
        assert!(apart > 0.5, "relief should move the frame: {apart}");
    }

    // --- the frame is clipped against the map as a polygon ---

    /// Signed-area-free area of a polygon (shoelace).
    fn area(poly: &[(f32, f32)]) -> f64 {
        let n = poly.len();
        let twice: f64 = (0..n)
            .map(|i| {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                f64::from(a.0) * f64::from(b.1) - f64::from(b.0) * f64::from(a.1)
            })
            .sum();
        twice.abs() / 2.0
    }

    /// Even-odd test: is the point inside the polygon?
    fn inside(poly: &[(f32, f32)], x: f64, z: f64) -> bool {
        let n = poly.len();
        let mut odd = false;
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let (ax, az, bx, bz) = (
                f64::from(a.0),
                f64::from(a.1),
                f64::from(b.0),
                f64::from(b.1),
            );
            if (az > z) != (bz > z) && x < ax + (z - az) / (bz - az) * (bx - ax) {
                odd = !odd;
            }
        }
        odd
    }

    /// The area of (quad intersect map), counted independently of the
    /// clipper: one sample per square metre of the map.
    fn sampled_overlap(quad: &[(f32, f32)], map: (u32, u32)) -> f64 {
        let mut count = 0_u32;
        for z in 0..map.1 {
            for x in 0..map.0 {
                if inside(quad, f64::from(x) + 0.5, f64::from(z) + 0.5) {
                    count += 1;
                }
            }
        }
        f64::from(count)
    }

    #[test]
    fn clip_to_rect_keeps_a_polygon_inside_and_drops_one_outside() {
        let inner = [(2.0, 2.0), (8.0, 3.0), (7.0, 9.0), (1.0, 6.0)];
        assert_eq!(clip_to_rect(&inner, (10.0, 10.0)), inner);
        let far = [(20.0, 20.0), (30.0, 20.0), (30.0, 30.0)];
        assert!(clip_to_rect(&far, (10.0, 10.0)).is_empty());
        // Touching only at a corner or along an edge leaves no area.
        let corner = [(10.0, 10.0), (20.0, 10.0), (20.0, 20.0)];
        assert!(clip_to_rect(&corner, (10.0, 10.0)).is_empty());
        // A square beside the map that shares part of one edge with it: east,
        // west, south and north edge.
        for (x, z) in [(10.0, 2.0), (-6.0, 2.0), (2.0, 10.0), (2.0, -6.0)] {
            let beside = [(x, z), (x + 6.0, z), (x + 6.0, z + 6.0), (x, z + 6.0)];
            let clipped = clip_to_rect(&beside, (10.0, 10.0));
            assert!(
                clipped.is_empty(),
                "beside the map at ({x}, {z}): {clipped:?}"
            );
        }
        assert!(clip_to_rect(&[], (10.0, 10.0)).is_empty());
    }

    /// A square pushed over each edge and each corner of a 10 x 10 map is cut
    /// to the part on the map, with its vertices exactly on the map's edges.
    #[test]
    fn clip_to_rect_cuts_at_every_edge_and_corner() {
        let square = |x: f32, z: f32| [(x, z), (x + 6.0, z), (x + 6.0, z + 6.0), (x, z + 6.0)];
        // (square corner, expected area on the map)
        let cases = [
            ((-3.0, 2.0), 3.0 * 6.0),  // past the west edge
            ((7.0, 2.0), 3.0 * 6.0),   // past the east edge
            ((2.0, -3.0), 6.0 * 3.0),  // past the north edge
            ((2.0, 7.0), 6.0 * 3.0),   // past the south edge
            ((-3.0, -3.0), 3.0 * 3.0), // past the north-west corner
            ((7.0, -3.0), 3.0 * 3.0),  // north-east
            ((7.0, 7.0), 3.0 * 3.0),   // south-east
            ((-3.0, 7.0), 3.0 * 3.0),  // south-west
            ((-3.0, -3.0), 9.0),       // (again: a corner case is a rectangle)
        ];
        for ((x, z), want) in cases {
            let poly = clip_to_rect(&square(x, z), (10.0, 10.0));
            assert!((area(&poly) - want).abs() < 1e-9, "{x},{z}: {poly:?}");
            for &(px, pz) in &poly {
                assert!((0.0..=10.0).contains(&px) && (0.0..=10.0).contains(&pz));
            }
            assert_eq!(poly.len(), 4, "{x},{z}: {poly:?}");
        }
        // A frame wider than the map on both sides keeps both cuts and the
        // corner of the map between them: 8 points at most.
        let wide = [(-5.0, 4.0), (15.0, 4.0), (15.0, 12.0), (-5.0, 12.0)];
        let poly = clip_to_rect(&wide, (10.0, 10.0));
        assert!((area(&poly) - 60.0).abs() < 1e-9, "{poly:?}");
        // A diamond whose points stick out of all four edges and whose sides
        // cut all four corners of the map off: an octagon (the maximum of 8).
        let diamond = [(5.0, -3.0), (13.0, 5.0), (5.0, 13.0), (-3.0, 5.0)];
        let poly = clip_to_rect(&diamond, (10.0, 10.0));
        assert_eq!(poly.len(), 8, "{poly:?}");
        assert!((area(&poly) - 92.0).abs() < 1e-9, "{poly:?}");
        // The diamond through the corners covers the whole map: its square.
        let through = [(5.0, -5.0), (15.0, 5.0), (5.0, 15.0), (-5.0, 5.0)];
        assert_eq!(
            clip_to_rect(&through, (10.0, 10.0)),
            [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        );
    }

    /// The camera over each edge and each corner of a map: the polygon is
    /// the quad's overlap with the map (area counted independently), lies on
    /// the map, and its vertices on the cut are exactly on the map's edge.
    #[test]
    fn fov_polygon_is_the_overlap_of_the_view_and_the_map() {
        let map = 256.0_f32;
        let size = (1280_u32, 720_u32);
        let m = f64::from(map);
        // (focus, side(s) that must be crossed: west, east, north, south)
        let cases = [
            ((0.0, 128.0), "west"),
            ((m, 128.0), "east"),
            ((128.0, 0.0), "north"),
            ((128.0, m), "south"),
            ((0.0, 0.0), "north-west"),
            ((m, 0.0), "north-east"),
            ((m, m), "south-east"),
            ((0.0, m), "south-west"),
        ];
        for (focus, name) in cases {
            for zoom in [0.3, 0.5] {
                let cam = camera(m, zoom, focus, size);
                let quad = fov_quad(&cam, (size.0 as f32, size.1 as f32));
                let past = |f: fn(f32) -> bool, axis: usize| {
                    quad.iter().any(|p| f(if axis == 0 { p.0 } else { p.1 }))
                };
                let crossed = [
                    ("west", past(|v| v < 0.0, 0)),
                    ("east", past(|v| v > 256.0, 0)),
                    ("north", past(|v| v < 0.0, 1)),
                    ("south", past(|v| v > 256.0, 1)),
                ];
                for part in name.split('-') {
                    assert!(
                        crossed.iter().any(|&(side, c)| side == part && c),
                        "{name} zoom {zoom}: the view must reach past the {part} edge: {quad:?}"
                    );
                }
                let poly = fov_polygon(&cam, (size.0 as f32, size.1 as f32), (map, map));
                assert!(poly.len() >= 3 && poly.len() <= 8, "{name}: {poly:?}");
                for &(x, z) in &poly {
                    assert!(
                        (0.0..=map).contains(&x) && (0.0..=map).contains(&z),
                        "{name}: ({x}, {z}) is off the map"
                    );
                }
                let want = sampled_overlap(&quad, (256, 256));
                let got = area(&poly);
                assert!(
                    (got - want).abs() <= want * 0.01 + 20.0,
                    "{name} zoom {zoom}: polygon area {got} vs the overlap counted {want}"
                );
                // Per-corner clamping would not do this: the clamped quad
                // differs from the overlap.
                let clamped = quad.map(|(x, z)| (x.clamp(0.0, map), z.clamp(0.0, map)));
                assert!(
                    (area(&clamped) - want).abs() > 1.0,
                    "{name} zoom {zoom}: the view is cut by an edge, so the clamp must be wrong"
                );
                // The cut runs along the map's edge: some vertices lie on it.
                let on_edge = poly
                    .iter()
                    .filter(|&&(x, z)| x == 0.0 || x == map || z == 0.0 || z == map)
                    .count();
                assert!(on_edge >= 2, "{name} zoom {zoom}: {poly:?}");
            }
        }
    }

    /// A view wholly on the map is the quad itself; a view of the whole map
    /// from far away covers the map and is the map's corners; a view that
    /// does not touch the map gives no frame.
    #[test]
    fn fov_polygon_is_the_quad_inside_the_map_and_empty_outside_it() {
        let size = (1280_u32, 720_u32);
        let cam = camera(40000.0, 0.2, (20000.0, 20000.0), size);
        let quad = fov_quad(&cam, (1280.0, 720.0));
        assert_eq!(
            fov_polygon(&cam, (1280.0, 720.0), (40000.0, 40000.0)),
            quad.to_vec()
        );
        // The camera's focus is limited to the map plus a margin; a map that
        // is far smaller than the view still gets a frame that covers it.
        let small = camera(256.0, 1.0, (128.0, 128.0), size);
        let poly = fov_polygon(&small, (1280.0, 720.0), (256.0, 256.0));
        assert!(
            poly.len() >= 4 && area(&poly) > 0.5 * 256.0 * 256.0,
            "{poly:?}"
        );
        // Panned far off the map the frame stays on the map or is empty.
        let off = camera(256.0, 0.0, (-500.0, -500.0), size);
        for (x, z) in fov_polygon(&off, (1280.0, 720.0), (256.0, 256.0)) {
            assert!((0.0..=256.0).contains(&x) && (0.0..=256.0).contains(&z));
        }
        // A frame somewhere else entirely: nothing to draw.
        let elsewhere = camera(40000.0, 0.2, (30000.0, 30000.0), size);
        assert!(fov_polygon(&elsewhere, (1280.0, 720.0), (256.0, 256.0)).is_empty());
    }

    #[test]
    fn a_flat_camera_still_gives_a_finite_quad() {
        let cfg = CameraConfig {
            map_size: DVec2::new(512.0, 512.0),
            pitch_near: 12_f64.to_radians(),
            pitch_far: 12_f64.to_radians(),
            fov_y: 60_f64.to_radians(),
            margin: 1000.0,
            ..CameraConfig::default()
        };
        let mut cam = Camera::new(cfg, 1280, 720);
        cam.set_zoom(0.5);
        cam.set_focus(DVec2::new(256.0, 256.0));
        assert!(
            cam.ground_point(DVec2::new(640.0, 0.0)).is_none(),
            "the top edge is above the horizon"
        );
        let q = fov_quad(&cam, (1280.0, 720.0));
        for (x, z) in q {
            assert!(x.is_finite() && z.is_finite());
        }
        assert!(q[0].1 < q[3].1, "still further north at the top");
        for (x, z) in fov_polygon(&cam, (1280.0, 720.0), (512.0, 512.0)) {
            assert!((0.0..=512.0).contains(&x) && (0.0..=512.0).contains(&z));
        }
    }

    #[test]
    fn bake_gives_a_whole_number_of_pixels_per_cell() {
        // 64 x 48 m = 16 x 12 gameplay cells -> 16 px per cell.
        let map = generate(64, 48, 1);
        let img = bake_terrain(&map, 256);
        assert_eq!((img.width, img.height), (256, 192));
        assert_eq!(img.rgba.len(), 256 * 192 * 4);
        assert!(img.rgba.chunks_exact(4).all(|p| p[3] == 255));
        let distinct: std::collections::BTreeSet<_> = img
            .rgba
            .chunks_exact(4)
            .map(|p| (p[0], p[1], p[2]))
            .collect();
        assert!(
            distinct.len() > 8,
            "terrain is not a flat color: {} colors",
            distinct.len()
        );
        assert_eq!(img, bake_terrain(&map, 256), "baking is deterministic");
        // Every pixel of one cell has the colour of the cell.
        for (cx, cz) in [(0usize, 0usize), (5, 7), (15, 11)] {
            let first = &img.rgba[((cz * 16) * 256 + cx * 16) * 4..][..4];
            for dy in 0..16 {
                for dx in 0..16 {
                    let at = ((cz * 16 + dy) * 256 + cx * 16 + dx) * 4;
                    assert_eq!(&img.rgba[at..at + 4], first, "cell ({cx}, {cz})");
                }
            }
        }
    }

    #[test]
    fn bake_downsizes_big_maps_to_the_requested_side() {
        // 256 x 128 m = 64 x 32 cells.
        let map = generate(256, 128, 2);
        let img = bake_terrain(&map, 64);
        assert_eq!((img.width, img.height), (64, 32));
        let same = bake_terrain(&map, 256);
        assert_eq!((same.width, same.height), (256, 128));
        // A 250 x 125 cell map is sampled down to a side of 100 px.
        let big = generate_cells(250, 125, 2);
        let img = bake_terrain(&big, 100);
        assert_eq!((img.width, img.height), (100, 50));
    }

    /// A map of `wc x hc` gameplay cells with the given terrain ids and a
    /// height function over the 1 m vertices (raw u16 heights).
    fn map_with_types(wc: u32, hc: u32, ids: Vec<u8>, raw_height: impl Fn(u32, u32) -> u16) -> Map {
        let (w, h) = (wc * CELL_METRES, hc * CELL_METRES);
        let heights: Vec<u16> = (0..=h)
            .flat_map(|z| (0..=w).map(move |x| (x, z)))
            .map(|(x, z)| raw_height(x, z))
            .collect();
        Map::from_parts(
            w,
            h,
            8.0,
            vec!["grass".into()],
            heights,
            vec![0; (w * h) as usize],
        )
        .unwrap()
        .with_terrain(
            sf_data::terrain::TerrainLayer::new(wc, hc, ids).unwrap(),
            &TerrainTypes::embedded(),
        )
        .unwrap()
    }

    fn texel(img: &MinimapImage, x: u32, y: u32) -> [u8; 3] {
        let at = ((y * img.width + x) * 4) as usize;
        [img.rgba[at], img.rgba[at + 1], img.rgba[at + 2]]
    }

    /// WP2.3: on a small map with every terrain type, the texel of a cell
    /// has the colour of its type, at one pixel per cell and at several.
    #[test]
    fn a_texel_has_the_colour_of_its_cells_terrain_type() {
        // 6 x 3 cells: row 0 = ids 0..=5, row 1 = reversed, row 2 = all water.
        let mut ids: Vec<u8> = (0..6).collect();
        ids.extend((0..6).rev());
        ids.extend([2; 6]);
        let map = map_with_types(6, 3, ids.clone(), |_, _| 0);
        for max_side in [6, 12, 24, 96] {
            let img = bake_terrain(&map, max_side);
            let s = max_side / 6;
            assert_eq!((img.width, img.height), (6 * s, 3 * s), "side {max_side}");
            for cz in 0..3u32 {
                for cx in 0..6u32 {
                    let want = mini(ids[(cz * 6 + cx) as usize]);
                    for (dx, dy) in [(0, 0), (s - 1, s - 1), (s / 2, s / 2)] {
                        assert_eq!(
                            texel(&img, cx * s + dx, cz * s + dy),
                            want,
                            "cell ({cx}, {cz}) at {s} px per cell"
                        );
                    }
                }
            }
        }
        // Every pair of types gives a different texel.
        let img = bake_terrain(&map, 6);
        let firsts: std::collections::BTreeSet<_> = (0..6).map(|x| texel(&img, x, 0)).collect();
        assert_eq!(firsts.len(), 6);
    }

    #[test]
    fn the_relief_shades_the_type_colour_lightly_and_never_changes_the_type() {
        let ids = vec![0u8; 8 * 3];
        let grass = mini(0);
        let lum = |c: [u8; 3]| u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2]);
        // Ground that rises to the east faces away from the north-west light
        // (darker); ground that falls to the east faces it (brighter).
        let rising = map_with_types(8, 3, ids.clone(), |x, _| (x * 1000) as u16);
        let falling = map_with_types(8, 3, ids.clone(), |x, _| ((32 - x) * 1000) as u16);
        let flat = map_with_types(8, 3, ids, |_, _| 5000);
        let (r, f, l) = (
            texel(&bake_terrain(&rising, 8), 4, 1),
            texel(&bake_terrain(&falling, 8), 4, 1),
            texel(&bake_terrain(&flat, 8), 4, 1),
        );
        assert_eq!(l, grass, "flat ground is the plain type colour");
        assert!(lum(r) < lum(l) && lum(l) < lum(f), "{r:?} {l:?} {f:?}");
        for c in [r, f] {
            for i in 0..3 {
                let ratio = f32::from(c[i]) / f32::from(grass[i]);
                assert!(
                    (1.0 - RELIEF_SHADE - 0.01..=1.0 + RELIEF_SHADE + 0.01).contains(&ratio),
                    "shade {ratio} beyond the limit"
                );
            }
        }
    }

    #[test]
    fn a_map_without_a_terrain_layer_falls_back_to_the_material_colour() {
        let map = Map::from_parts(
            16,
            16,
            8.0,
            vec!["grass".into()],
            vec![0; 17 * 17],
            vec![0; 16 * 16],
        )
        .unwrap();
        assert!(map.terrain().is_none());
        let img = bake_terrain(&map, 4);
        assert_eq!((img.width, img.height), (4, 4));
        let want = material_colors(&map)[0];
        assert_eq!(texel(&img, 2, 2), want);
    }

    #[test]
    fn a_type_the_table_lacks_is_grey_not_a_panic() {
        let map = map_with_types(2, 1, vec![0, 3], |_, _| 0);
        let only_grass =
            TerrainTypes::new(vec![TerrainTypes::embedded().get(0).unwrap().clone()]).unwrap();
        let img = bake_terrain_with(&map, 2, &only_grass, &TerrainLooks::embedded());
        assert_eq!(texel(&img, 1, 0), UNKNOWN);
        assert_eq!(texel(&img, 0, 0), mini(0));
    }

    /// The shipped bridge map has water, rock, road and a bridge: all of them
    /// show on its minimap, and the bridge is not swallowed by the water.
    #[test]
    fn the_bridge_map_shows_water_rock_and_a_bridge() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/bridge");
        let map = Map::load(&dir).expect("bridge map");
        let types = TerrainTypes::embedded();
        let img = bake_terrain(&map, 256);
        let (cw, ch) = map.size_in_cells();
        let s = img.width / cw;
        let layer = map.terrain().expect("terrain layer");
        for name in ["water", "rock", "bridge"] {
            let id = types.id_of(name).unwrap();
            let (cx, cz) = (0..ch)
                .flat_map(|z| (0..cw).map(move |x| (x, z)))
                .find(|&(x, z)| layer.get(x, z) == Some(id))
                .unwrap_or_else(|| panic!("the bridge map has no {name} cell"));
            let want = mini(id);
            let got = texel(&img, cx * s + s / 2, cz * s + s / 2);
            for i in 0..3 {
                let ratio = f32::from(got[i]) / f32::from(want[i]);
                assert!(
                    (1.0 - RELIEF_SHADE - 0.01..=1.0 + RELIEF_SHADE + 0.01).contains(&ratio),
                    "{name} cell ({cx}, {cz}): {got:?} vs {want:?}"
                );
            }
        }
    }

    /// Review WP2.3 round 1: a map side that is not a multiple of 4 m ends in
    /// a partial cell, and the image (whole cells) is wider than the map. Every
    /// cell must still be drawn where its metres are, and the unit dots, the
    /// field-of-view frame and a click must land on the same cell as the image.
    /// With the whole image stretched over the map (the round-0 bug) cells near
    /// the far edges show their neighbour.
    #[test]
    fn a_map_with_partial_cells_draws_every_cell_in_its_own_place() {
        const SCREEN: (u32, u32) = (1200, 1200);
        for (w, h, side) in [
            (66_u32, 50_u32, 256_u32),
            (66, 50, 17),
            (130, 62, 256),
            (1022, 1022, 256),
        ] {
            let (cw, ch) = (w.div_ceil(CELL_METRES), h.div_ceil(CELL_METRES));
            // Neighbours always differ: +1 per column, +2 per row (mod 6).
            let ids: Vec<u8> = (0..ch)
                .flat_map(|z| (0..cw).map(move |x| ((x + 2 * z) % 6) as u8))
                .collect();
            let map = Map::from_parts(
                w,
                h,
                8.0,
                vec!["grass".into()],
                vec![0; ((w + 1) * (h + 1)) as usize],
                vec![0; (w * h) as usize],
            )
            .unwrap()
            .with_terrain(
                sf_data::terrain::TerrainLayer::new(cw, ch, ids.clone()).unwrap(),
                &TerrainTypes::embedded(),
            )
            .unwrap();
            assert_eq!(map.size_in_cells(), (cw, ch));
            let minimap = Minimap::bake(&map, side);
            let img = minimap.image();
            let frame = Rect::new(20.0, 20.0, 1100.0, 1100.0);
            let step = (cw / 24).max(1);
            let cells: Vec<(u32, u32)> = (0..ch)
                .step_by(step as usize)
                .chain([ch - 1])
                .flat_map(|z| {
                    (0..cw)
                        .step_by(step as usize)
                        .chain([cw - 1])
                        .map(move |x| (x, z))
                })
                .collect();
            // The part of cell (cx, cz) that lies on the map, in metres.
            let span = |c: u32, size: u32| {
                let a = (c * CELL_METRES) as f32;
                (a, ((c + 1) * CELL_METRES).min(size) as f32)
            };
            // Three points across every sampled cell, each a dot of its own colour.
            let fractions = [0.15_f32, 0.5, 0.85];
            let mut dots = Vec::new();
            for &(cx, cz) in &cells {
                let ((x0, x1), (z0, z1)) = (span(cx, w), span(cz, h));
                for fx in fractions {
                    for fz in fractions {
                        let i = dots.len() as u32;
                        let colour = Color((i & 0xff) as u8, ((i >> 8) & 0xff) as u8, 7, 255);
                        let p = (x0 + (x1 - x0) * fx, z0 + (z1 - z0) * fz);
                        dots.push(((cx, cz), p, colour));
                    }
                }
            }
            // The field of view: exactly the last (partial) cell.
            let (lx, lz) = (span(cw - 1, w), span(ch - 1, h));
            let fov = [(lx.0, lz.0), (lx.1, lz.0), (lx.1, lz.1), (lx.0, lz.1)];
            // Click in the middle of the last cell.
            let click_world = ((lx.0 + lx.1) * 0.5, (lz.0 + lz.1) * 0.5);
            let mut ui = Ui::new(UiConfig::default());
            // Where would the click be: draw once to learn the quad.
            ui.begin_frame(UiInput::default(), SCREEN, 1.0);
            ui.minimap(&minimap, "m", frame, None, []);
            ui.end_frame();
            let quad = minimap_quad(&ui);
            let click = quad.world_to_screen(click_world, (w as f32, h as f32));
            let mut input = UiInput {
                cursor: Some(click),
                ..UiInput::default()
            };
            input.pressed[0] = true;
            input.down[0] = true;
            ui.begin_frame(input, SCREEN, 1.0);
            let target = ui.minimap(
                &minimap,
                "m",
                frame,
                Some(&fov),
                dots.iter().map(|&(_, p, c)| (p, c)),
            );
            ui.end_frame();
            let quad = minimap_quad(&ui);
            let list = ui.draw_list();
            // The image under every dot is the colour of the dot's cell.
            let mut wrong = Vec::new();
            for &((cx, cz), p, colour) in &dots {
                let c = colour.to_array();
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for v in list.vertices.iter().filter(|v| v.color == c) {
                    x0 = x0.min(v.pos[0]);
                    y0 = y0.min(v.pos[1]);
                    x1 = x1.max(v.pos[0]);
                    y1 = y1.max(v.pos[1]);
                }
                assert!(
                    x0 <= x1,
                    "{w}x{h}: dot of cell ({cx}, {cz}) at {p:?} is not drawn"
                );
                let centre = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
                // A dot is snapped to whole pixels; less than a pixel off.
                let exact = quad.world_to_screen(p, (w as f32, h as f32));
                assert!(
                    (centre.0 - exact.0).abs() <= 0.5 && (centre.1 - exact.1).abs() <= 0.5,
                    "{w}x{h}: dot of cell ({cx}, {cz}) at {centre:?}, the image has it at {exact:?}"
                );
                let (u, v) = quad.texel(exact);
                let want = mini(ids[(cz * cw + cx) as usize]);
                if texel(img, u, v) != want {
                    wrong.push((cx, cz));
                }
            }
            wrong.sort_unstable();
            wrong.dedup();
            assert!(
                wrong.is_empty(),
                "{w}x{h} at side {side}: {} cells show another cell's colour, e.g. {:?}",
                wrong.len(),
                &wrong[..wrong.len().min(5)]
            );
            // The frame of the field of view sits on the borders of the last
            // cell in the image: its edges map to that cell's first texel and
            // to the end of the image.
            let fov_colour = UiConfig::default().fov_frame.to_array();
            let frame_pts: Vec<(f32, f32)> = list
                .vertices
                .iter()
                .filter(|v| v.color == fov_colour)
                .map(|v| (v.pos[0], v.pos[1]))
                .collect();
            assert!(!frame_pts.is_empty(), "{w}x{h}: no field-of-view frame");
            let left = frame_pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
            let right = frame_pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
            let top = frame_pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
            let bottom = frame_pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
            // The line is a pixel thick around the exact edge.
            let (ul, vt) = quad.texel_f((left + 0.5, top + 0.5));
            let (ur, vb) = quad.texel_f((right - 0.5, bottom - 0.5));
            let (px_x, px_y) = (img.width as f32 / cw as f32, img.height as f32 / ch as f32);
            let tol = quad.texels_per_pixel() + 0.01;
            for (got, want, what) in [
                (ul, (cw - 1) as f32 * px_x, "left"),
                (vt, (ch - 1) as f32 * px_y, "top"),
                (ur, minimap.uv()[2], "right"),
                (vb, minimap.uv()[3], "bottom"),
            ] {
                assert!(
                    (got - want).abs() <= tol,
                    "{w}x{h}: the {what} edge of the frame is at texel {got}, the cell's at {want}"
                );
            }
            // The click in the last cell moves the camera into it.
            let (tx, tz) = target.expect("a click on the minimap");
            assert_eq!(
                (
                    (tx / CELL_METRES as f32) as u32,
                    (tz / CELL_METRES as f32) as u32
                ),
                (cw - 1, ch - 1),
                "{w}x{h}: click at {click:?} went to ({tx}, {tz})"
            );
        }
    }

    /// The minimap image as drawn: its screen rectangle and texel rectangle.
    struct DrawnQuad {
        pos: [f32; 4],
        uv: [f32; 4],
    }

    impl DrawnQuad {
        /// The screen point of a world point, assuming the quad shows the
        /// whole map (what the dots and the frame use).
        fn world_to_screen(&self, p: (f32, f32), map: (f32, f32)) -> (f32, f32) {
            (
                self.pos[0] + p.0 / map.0 * (self.pos[2] - self.pos[0]),
                self.pos[1] + p.1 / map.1 * (self.pos[3] - self.pos[1]),
            )
        }

        /// The (continuous) texel coordinate the GPU samples at a screen point.
        fn texel_f(&self, s: (f32, f32)) -> (f32, f32) {
            (
                self.uv[0]
                    + (s.0 - self.pos[0]) / (self.pos[2] - self.pos[0]) * (self.uv[2] - self.uv[0]),
                self.uv[1]
                    + (s.1 - self.pos[1]) / (self.pos[3] - self.pos[1]) * (self.uv[3] - self.uv[1]),
            )
        }

        fn texel(&self, s: (f32, f32)) -> (u32, u32) {
            let (u, v) = self.texel_f(s);
            (u.floor() as u32, v.floor() as u32)
        }

        fn texels_per_pixel(&self) -> f32 {
            (self.uv[2] - self.uv[0]) / (self.pos[2] - self.pos[0])
        }
    }

    fn minimap_quad(ui: &Ui) -> DrawnQuad {
        let list = ui.draw_list();
        let batch = list
            .batches
            .iter()
            .find(|b| b.texture == Tex::Minimap)
            .expect("the minimap image is drawn");
        let vs: Vec<_> = list.indices[batch.start as usize..batch.end as usize]
            .iter()
            .map(|&i| list.vertices[i as usize])
            .collect();
        let min = |f: fn(&crate::draw::Vertex) -> f32| vs.iter().map(f).fold(f32::MAX, f32::min);
        let max = |f: fn(&crate::draw::Vertex) -> f32| vs.iter().map(f).fold(f32::MIN, f32::max);
        DrawnQuad {
            pos: [
                min(|v| v.pos[0]),
                min(|v| v.pos[1]),
                max(|v| v.pos[0]),
                max(|v| v.pos[1]),
            ],
            uv: [
                min(|v| v.uv[0]),
                min(|v| v.uv[1]),
                max(|v| v.uv[0]),
                max(|v| v.uv[1]),
            ],
        }
    }
}
