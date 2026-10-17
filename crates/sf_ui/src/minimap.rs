//! The minimap: a baked top-down terrain image, unit dots, the camera's
//! field of view, and click/drag to move the camera.
//!
//! # Rendering to a texture
//! The terrain image is *baked on the CPU* by [`Minimap::bake`]: one pixel per
//! cell (or fewer for maps larger than the requested side), coloured with the
//! average of the cell's procedural material texture and shaded by the slope
//! of the heightmap (light from the north-west). Baking the map is a few
//! milliseconds for 256 x 256 and happens once, when the map is set; the
//! result is uploaded as a texture by `UiRenderer::set_minimap`. The terrain
//! is static in this work package, so there is no update rate: call
//! [`Minimap::bake`] and `set_minimap` again when the map changes (terrain
//! edits, WP2.x). A GPU top-down render of the real terrain pass was
//! rejected: it would tie the minimap to the terrain chunks and LOD of WP1.8
//! and cost a depth buffer and a second scene pass for an image that never
//! changes.
//!
//! Everything dynamic is drawn on top as UI quads every frame: unit dots
//! (a few pixels each) and the field-of-view frame.
//!
//! # Coordinates
//! The map is `map_w x map_h` metres with `x` east and `z` south; north is
//! up on the minimap, like on screen. The map is centred in the panel with
//! its aspect ratio kept ([`MinimapGeometry`]).

use sf_data::map::Map;
use sf_render::Camera;
use sf_render::camera::DVec2;

use crate::color::Color;
use crate::draw::Texture;
use crate::layout::Rect;
use crate::ui::Ui;

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
}

impl Minimap {
    /// Bake `map` into an image whose longer side is at most `max_side`
    /// pixels.
    pub fn bake(map: &Map, max_side: u32) -> Self {
        Self {
            image: bake_terrain(map, max_side),
            map_size: (map.width() as f32, map.height() as f32),
        }
    }

    pub fn image(&self) -> &MinimapImage {
        &self.image
    }

    pub fn map_size(&self) -> (f32, f32) {
        self.map_size
    }
}

/// Average sRGB color of each material of the map.
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

/// See [`Minimap::bake`].
pub fn bake_terrain(map: &Map, max_side: u32) -> MinimapImage {
    let (mw, mh) = (map.width(), map.height());
    let longest = mw.max(mh);
    let (w, h) = if longest <= max_side.max(1) {
        (mw, mh)
    } else {
        let k = f64::from(max_side.max(1)) / f64::from(longest);
        (
            ((f64::from(mw) * k).round() as u32).max(1),
            ((f64::from(mh) * k).round() as u32).max(1),
        )
    };
    let colors = material_colors(map);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for py in 0..h {
        for px in 0..w {
            // The cell this pixel samples (nearest, centre of the pixel).
            let cx = (u64::from(px) * 2 + 1) * u64::from(mw) / (u64::from(w) * 2);
            let cz = (u64::from(py) * 2 + 1) * u64::from(mh) / (u64::from(h) * 2);
            let (cx, cz) = (cx as i64, cz as i64);
            let base = colors
                .get(usize::from(map.material_at(cx, cz)))
                .copied()
                .unwrap_or([128, 128, 128]);
            // North-west light: higher towards the light side = brighter.
            let slope = map.vertex_height(cx, cz) - map.vertex_height(cx + 1, cz + 1);
            let shade = (1.0 + slope * 0.35).clamp(0.7, 1.3);
            for c in base {
                rgba.push((f32::from(c) * shade).clamp(0.0, 255.0).round() as u8);
            }
            rgba.push(255);
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
        let img = minimap.image();
        self.image(
            Texture::Minimap,
            content,
            [0.0, 0.0, img.width as f32, img.height as f32],
            Color::WHITE,
        );

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
    use sf_data::map::generate::generate;
    use sf_render::CameraConfig;

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
    fn bake_gives_one_pixel_per_cell_with_shaded_material_colors() {
        let map = generate(64, 48, 1);
        let img = bake_terrain(&map, 256);
        assert_eq!((img.width, img.height), (64, 48));
        assert_eq!(img.rgba.len(), 64 * 48 * 4);
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
    }

    #[test]
    fn bake_downsizes_big_maps_to_the_requested_side() {
        let map = generate(256, 128, 2);
        let img = bake_terrain(&map, 64);
        assert_eq!((img.width, img.height), (64, 32));
        let same = bake_terrain(&map, 256);
        assert_eq!((same.width, same.height), (256, 128));
    }
}
