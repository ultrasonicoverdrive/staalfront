//! Instanced drawing of many small objects (units): interpolation between two
//! sim ticks, bucketing by map tile, frustum culling per tile, one instance
//! list per mesh kind.
//!
//! # Data flow (PLAN.md section 1: sim -> render is one way)
//! * Once per **sim tick** the game hands the [`Instancer`] one [`Motion`] per
//!   entity ([`Instancer::update_tick`]). A `Motion` holds the previous and
//!   current position and facing already converted to `f32` through
//!   `sf_math::render_bridge` (the only fixed-point to float conversion), so
//!   this crate never needs `sf_sim` and the per-frame work is float-only.
//!   The motions are bucketed by map tile (counting sort, O(n)).
//! * Once per **frame** [`Instancer::build`] takes the frustum of the camera
//!   and the interpolation factor `alpha`; tiles whose bounding box is
//!   outside the frustum are skipped, the entities of the visible tiles are
//!   interpolated (`lerp(prev, current, alpha)`, facing along the shortest
//!   arc), put on the ground (display only: the sim stays 2D) and appended
//!   to the instance list of their mesh kind.
//! * The caller uploads each list with `Renderer::set_instances`: one
//!   instance buffer and one draw call per mesh kind.
//!
//! # Tiles
//! The map is divided into square tiles of [`DEFAULT_TILE_SIZE`] m (32 m, the
//! same as a terrain chunk, so one tile's units sit on one chunk). An
//! entity belongs to the tile of its *current* position. The tile's box is
//! not the nominal tile square but the box around the previous and current
//! positions of all its members, grown by the largest shape footprint; since
//! an interpolated position always lies between the two, no member can
//! stick out of its tile's box. Its height range is the terrain relief
//! (`set_height_range`) plus the tallest shape, so units standing on hills
//! are never culled wrongly. Culling therefore never drops a unit that
//! would be (even partly) visible.

use std::f32::consts::TAU;

use glam::Vec3;
use sf_math::render_bridge::vec2_to_f32;
use sf_math::{Facing, Vec2};

use crate::frustum::{Aabb, Frustum};
use crate::scene::{Instance, MeshKind};
use crate::style::Style;

/// Tile edge in metres; equals the terrain chunk size (32 cells of 1 m).
pub const DEFAULT_TILE_SIZE: f32 = 32.0;

/// One entity's motion over the last sim tick, in render units (metres,
/// facing in 1/256 turns).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// Index into the style's shapes (wraps around).
    pub shape: u8,
    /// Index into the style's palette (wraps around): the owner.
    pub player: u8,
    prev: [f32; 2],
    pos: [f32; 2],
    prev_facing: u8,
    facing: u8,
}

impl Motion {
    /// From the fixed-point state of the previous and current tick. Cells are
    /// metres; the sim's `y` is the render `z`.
    pub fn from_fixed(
        shape: u8,
        player: u8,
        prev_pos: Vec2,
        pos: Vec2,
        prev_facing: Facing,
        facing: Facing,
    ) -> Self {
        Self {
            shape,
            player,
            prev: vec2_to_f32(prev_pos),
            pos: vec2_to_f32(pos),
            prev_facing: prev_facing.raw(),
            facing: facing.raw(),
        }
    }

    /// Position `(x, z)` and facing in steps at fraction `alpha` of the way
    /// from the previous to the current tick (clamped to `0..=1`). The
    /// returned facing is unwrapped (it may leave `0..256`): the previous
    /// facing plus the shortest signed turn times `alpha`, so 255 -> 1
    /// passes through 0, not through 128. `alpha = 0` is exactly the
    /// previous state, `alpha = 1` exactly the current one.
    pub fn at(&self, alpha: f32) -> ([f32; 2], f32) {
        let a = alpha.clamp(0.0, 1.0);
        let lerp = |p: f32, c: f32| p * (1.0 - a) + c * a;
        let pos = [
            lerp(self.prev[0], self.pos[0]),
            lerp(self.prev[1], self.pos[1]),
        ];
        let turn = Facing::new(self.prev_facing).signed_diff(Facing::new(self.facing));
        // Facing steps are small integers (0..=255): exact in f32.
        let steps = f32::from(self.prev_facing) + f32::from(turn) * a;
        (pos, steps)
    }

    /// The tile-box corner points: previous and current position.
    fn extent(&self) -> ([f32; 2], [f32; 2]) {
        (self.prev, self.pos)
    }
}

/// Model matrix of a shape standing at `(x, y, z)` facing `steps`
/// (1/256 turns; 0 = along `+x`, increasing turns towards `+z`), scaled by
/// `size` = `(along the facing, height, across)`.
pub fn model_matrix(x: f32, y: f32, z: f32, steps: f32, size: [f32; 3]) -> [[f32; 4]; 4] {
    let (s, c) = (steps * (TAU / 256.0)).sin_cos();
    [
        [c * size[0], 0.0, s * size[0], 0.0],
        [0.0, size[1], 0.0, 0.0],
        [-s * size[2], 0.0, c * size[2], 0.0],
        [x, y, z, 1.0],
    ]
}

/// What one [`Instancer::build`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InstanceStats {
    /// All entities known to the instancer.
    pub total: u32,
    /// Entities that went into the instance lists (inside visible tiles).
    pub visible: u32,
    /// Non-empty tiles.
    pub tiles_total: u32,
    /// Non-empty tiles that passed the frustum test.
    pub tiles_visible: u32,
    /// Visible entities per [`MeshKind::index`].
    pub per_mesh: [u32; MeshKind::ALL.len()],
}

impl InstanceStats {
    /// Draw calls needed for the visible instances: one per used mesh kind.
    pub fn draw_calls(&self) -> u32 {
        self.per_mesh.iter().filter(|&&n| n > 0).count() as u32
    }
}

/// Buckets motions by tile, culls tiles against a frustum and builds the
/// per-mesh instance lists. Buffers are reused between frames.
pub struct Instancer {
    style: Style,
    tile_size: f32,
    tiles_x: usize,
    tiles_z: usize,
    motions: Vec<Motion>,
    /// Entity indices grouped by tile (`tile_start[t]..tile_start[t + 1]`).
    order: Vec<u32>,
    tile_start: Vec<u32>,
    tile_boxes: Vec<Aabb>,
    footprint: f32,
    tallest: f32,
    ground: (f32, f32),
    lists: [Vec<Instance>; MeshKind::ALL.len()],
    stats: InstanceStats,
}

impl Instancer {
    /// For a map of `map_size` metres (x, z) with the default 32 m tiles.
    pub fn new(style: Style, map_size: (f32, f32)) -> Self {
        Self::with_tile_size(style, map_size, DEFAULT_TILE_SIZE)
    }

    pub fn with_tile_size(style: Style, map_size: (f32, f32), tile_size: f32) -> Self {
        assert!(tile_size > 0.0, "tile size must be positive");
        let tiles_x = (map_size.0 / tile_size).ceil().max(1.0) as usize;
        let tiles_z = (map_size.1 / tile_size).ceil().max(1.0) as usize;
        // Half the diagonal of the widest footprint covers any rotation.
        let footprint = style
            .shapes()
            .iter()
            .map(|s| 0.5 * s.size[0].hypot(s.size[2]))
            .fold(0.0, f32::max);
        let tallest = style.shapes().iter().map(|s| s.size[1]).fold(0.0, f32::max);
        Self {
            style,
            tile_size,
            tiles_x,
            tiles_z,
            motions: Vec::new(),
            order: Vec::new(),
            tile_start: vec![0; tiles_x * tiles_z + 1],
            tile_boxes: vec![Aabb::new(Vec3::ZERO, Vec3::ZERO); tiles_x * tiles_z],
            footprint,
            tallest,
            ground: (0.0, 0.0),
            lists: Default::default(),
            stats: InstanceStats::default(),
        }
    }

    pub fn style(&self) -> &Style {
        &self.style
    }

    pub fn tile_size(&self) -> f32 {
        self.tile_size
    }

    /// Lowest and highest terrain height (metres): the tile boxes get this
    /// vertical range so relief cannot hide a unit.
    pub fn set_height_range(&mut self, min: f32, max: f32) {
        self.ground = (min.min(max), max.max(min));
    }

    pub fn len(&self) -> usize {
        self.motions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.motions.is_empty()
    }

    /// Replace the entities (call once per sim tick) and bucket them.
    pub fn update_tick(&mut self, motions: impl IntoIterator<Item = Motion>) {
        self.motions.clear();
        self.motions.extend(motions);
        self.rebucket();
    }

    fn tile_of(&self, p: [f32; 2]) -> usize {
        let tx = ((p[0] / self.tile_size).max(0.0) as usize).min(self.tiles_x - 1);
        let tz = ((p[1] / self.tile_size).max(0.0) as usize).min(self.tiles_z - 1);
        tz * self.tiles_x + tx
    }

    fn rebucket(&mut self) {
        let tiles = self.tiles_x * self.tiles_z;
        self.tile_start.clear();
        self.tile_start.resize(tiles + 1, 0);
        for m in &self.motions {
            let t = self.tile_of(m.pos);
            self.tile_start[t + 1] += 1;
        }
        for t in 0..tiles {
            self.tile_start[t + 1] += self.tile_start[t];
        }
        // Fill with a moving cursor per tile.
        let mut cursor: Vec<u32> = self.tile_start[..tiles].to_vec();
        self.order.clear();
        self.order.resize(self.motions.len(), 0);
        let (lo_y, hi_y) = (self.ground.0, self.ground.1 + self.tallest);
        let mut lo = vec![[f32::MAX; 2]; tiles];
        let mut hi = vec![[f32::MIN; 2]; tiles];
        for (i, m) in self.motions.iter().enumerate() {
            let t = self.tile_of(m.pos);
            self.order[cursor[t] as usize] = i as u32;
            cursor[t] += 1;
            let (a, b) = m.extent();
            for p in [a, b] {
                lo[t] = [lo[t][0].min(p[0]), lo[t][1].min(p[1])];
                hi[t] = [hi[t][0].max(p[0]), hi[t][1].max(p[1])];
            }
        }
        for t in 0..tiles {
            if self.tile_start[t] == self.tile_start[t + 1] {
                continue;
            }
            let r = self.footprint;
            self.tile_boxes[t] = Aabb::new(
                Vec3::new(lo[t][0] - r, lo_y, lo[t][1] - r),
                Vec3::new(hi[t][0] + r, hi_y, hi[t][1] + r),
            );
        }
    }

    /// Cull tiles against `frustum` and rebuild the instance lists for the
    /// interpolation factor `alpha`. `ground(x, z)` is the terrain height
    /// the unit stands on (display only). Returns the counters.
    pub fn build(
        &mut self,
        frustum: &Frustum,
        alpha: f32,
        ground: impl Fn(f32, f32) -> f32,
    ) -> InstanceStats {
        for list in &mut self.lists {
            list.clear();
        }
        let mut stats = InstanceStats {
            total: self.motions.len() as u32,
            ..InstanceStats::default()
        };
        for t in 0..self.tiles_x * self.tiles_z {
            let (start, end) = (self.tile_start[t] as usize, self.tile_start[t + 1] as usize);
            if start == end {
                continue;
            }
            stats.tiles_total += 1;
            if !frustum.intersects(&self.tile_boxes[t]) {
                continue;
            }
            stats.tiles_visible += 1;
            for &i in &self.order[start..end] {
                let m = &self.motions[i as usize];
                let ([x, z], steps) = m.at(alpha);
                let shape = self.style.shape(m.shape);
                let [r, g, b] = self.style.linear(m.player);
                self.lists[shape.mesh.index()].push(Instance {
                    model: model_matrix(x, ground(x, z), z, steps, shape.size),
                    color: [r, g, b, 1.0],
                });
                stats.per_mesh[shape.mesh.index()] += 1;
                stats.visible += 1;
            }
        }
        self.stats = stats;
        stats
    }

    /// The instances of the last [`Instancer::build`] for one mesh kind.
    pub fn instances(&self, mesh: MeshKind) -> &[Instance] {
        &self.lists[mesh.index()]
    }

    /// The counters of the last build.
    pub fn stats(&self) -> InstanceStats {
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, CameraConfig, DVec2};
    use sf_math::Fx;

    fn v2(x: i32, y: i32) -> Vec2 {
        Vec2::from_ints(x, y)
    }

    fn motion(prev: Vec2, pos: Vec2, pf: u8, f: u8) -> Motion {
        Motion::from_fixed(0, 0, prev, pos, Facing::new(pf), Facing::new(f))
    }

    #[test]
    fn alpha_zero_is_previous_and_one_is_current() {
        let m = motion(v2(10, 20), v2(11, 22), 10, 30);
        assert_eq!(m.at(0.0), ([10.0, 20.0], 10.0));
        assert_eq!(m.at(1.0), ([11.0, 22.0], 30.0));
        let (p, f) = m.at(0.5);
        assert_eq!(p, [10.5, 21.0]);
        assert_eq!(f, 20.0);
        // Out-of-range alpha is clamped.
        assert_eq!(m.at(-3.0), m.at(0.0));
        assert_eq!(m.at(7.0), m.at(1.0));
    }

    #[test]
    fn fixed_point_fractions_convert_exactly() {
        let half = Fx::from_raw(Fx::ONE.raw() / 2);
        let m = motion(Vec2::new(half, half), Vec2::new(half, half), 0, 0);
        assert_eq!(m.at(0.3).0, [0.5, 0.5]);
    }

    #[test]
    fn facing_wraps_the_short_way() {
        // 255 -> 1 is a turn of +2 through 0.
        let m = motion(v2(0, 0), v2(0, 0), 255, 1);
        let (_, mid) = m.at(0.5);
        assert_eq!(mid.rem_euclid(256.0), 0.0);
        let (_, end) = m.at(1.0);
        assert_eq!(end.rem_euclid(256.0), 1.0);
        // And the other way: 1 -> 255 passes through 0 as well.
        let m = motion(v2(0, 0), v2(0, 0), 1, 255);
        assert_eq!(m.at(0.5).1.rem_euclid(256.0), 0.0);
        // Never takes the long way: 10 -> 200 turns backwards (-66 steps).
        let m = motion(v2(0, 0), v2(0, 0), 10, 200);
        assert!((m.at(0.5).1 - (10.0 - 33.0)).abs() < 1e-4);
    }

    #[test]
    fn model_matrix_points_along_the_facing() {
        let col = |m: [[f32; 4]; 4], i: usize| Vec3::new(m[i][0], m[i][1], m[i][2]);
        // Facing 0 = +x, 64 (a quarter turn) = +z.
        let m0 = model_matrix(1.0, 2.0, 3.0, 0.0, [2.0, 1.0, 1.0]);
        assert!((col(m0, 0) - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-6);
        assert_eq!(m0[3], [1.0, 2.0, 3.0, 1.0]);
        let m64 = model_matrix(0.0, 0.0, 0.0, 64.0, [2.0, 1.0, 1.0]);
        assert!((col(m64, 0) - Vec3::new(0.0, 0.0, 2.0)).length() < 1e-5);
        // A proper rotation: the across axis turns with it.
        assert!((col(m64, 2) - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5);
    }

    /// A deterministic spread of `n` entities on a `side` x `side` m map.
    fn crowd(n: usize, side: f32) -> Vec<Motion> {
        let per_row = (n as f32).sqrt().ceil() as usize;
        let step = side / per_row as f32;
        (0..n)
            .map(|i| {
                let (cx, cz) = ((i % per_row) as f32 * step, (i / per_row) as f32 * step);
                let (dx, dz) = (((i * 7) % 5) as f32 * 0.1, ((i * 3) % 5) as f32 * 0.1);
                let to_fx = |v: f32| sf_math::render_bridge::fx_from_f32(v);
                Motion::from_fixed(
                    (i % 3) as u8,
                    ((i / 3) % 4) as u8,
                    Vec2::new(to_fx(cx), to_fx(cz)),
                    Vec2::new(to_fx(cx + dx), to_fx(cz + dz)),
                    Facing::new((i % 256) as u8),
                    Facing::new(((i + 5) % 256) as u8),
                )
            })
            .collect()
    }

    fn camera(zoom: f64, focus: (f64, f64), side: f64) -> Camera {
        let cfg = CameraConfig {
            map_size: DVec2::new(side, side),
            ..CameraConfig::default()
        };
        let mut c = Camera::new(cfg, 1280, 720);
        c.set_zoom(zoom);
        c.set_focus(DVec2::new(focus.0, focus.1));
        c
    }

    fn instancer(n: usize, side: f32) -> Instancer {
        let mut inst = Instancer::new(Style::embedded(), (side, side));
        inst.set_height_range(0.0, 6.0);
        inst.update_tick(crowd(n, side));
        inst
    }

    #[test]
    fn zooming_in_culls_most_instances() {
        let mut inst = instancer(10_000, 256.0);
        let far = camera(1.0, (128.0, 128.0), 256.0);
        let near = camera(0.2, (128.0, 128.0), 256.0);
        let all = inst.build(&Frustum::from_view_proj(far.view_proj()), 0.5, |_, _| 0.0);
        assert_eq!(all.total, 10_000);
        assert_eq!(all.visible, 10_000, "whole map in view");
        assert_eq!(all.tiles_visible, all.tiles_total);
        let close = inst.build(&Frustum::from_view_proj(near.view_proj()), 0.5, |_, _| 0.0);
        assert!(close.visible > 0);
        assert!(
            close.visible * 4 < all.visible,
            "zoomed in {} vs {}",
            close.visible,
            all.visible
        );
        assert!(close.tiles_visible < close.tiles_total);
        assert_eq!(close.per_mesh.iter().sum::<u32>(), close.visible);
        // Lists match the counters.
        let listed: usize = MeshKind::ALL.iter().map(|&k| inst.instances(k).len()).sum();
        assert_eq!(listed as u32, close.visible);
    }

    #[test]
    fn culling_never_drops_an_instance_that_is_in_view() {
        let side = 256.0;
        let mut inst = instancer(4_000, side);
        let motions = crowd(4_000, side);
        for (zoom, focus) in [
            (0.2, (60.0, 90.0)),
            (0.35, (200.0, 30.0)),
            (0.1, (10.0, 10.0)),
        ] {
            let cam = camera(zoom, focus, f64::from(side));
            let frustum = Frustum::from_view_proj(cam.view_proj());
            let alpha = 0.37;
            let stats = inst.build(&frustum, alpha, |_, _| 0.0);
            let in_view = motions
                .iter()
                .filter(|m| {
                    let ([x, z], _) = m.at(alpha);
                    frustum.contains_point(Vec3::new(x, 0.5, z))
                })
                .count() as u32;
            assert!(in_view > 0, "test view shows nothing");
            assert!(stats.visible >= in_view, "{} < {in_view}", stats.visible);
        }
    }

    #[test]
    fn built_instances_carry_the_player_colors_and_one_list_per_mesh() {
        let mut inst = instancer(1_200, 128.0);
        let cam = camera(1.0, (64.0, 64.0), 128.0);
        let stats = inst.build(&Frustum::from_view_proj(cam.view_proj()), 0.0, |_, _| 0.0);
        assert_eq!(stats.draw_calls(), 3);
        let style = Style::embedded();
        for kind in [MeshKind::Cube, MeshKind::Cylinder, MeshKind::Cone] {
            assert_eq!(inst.instances(kind).len(), 400);
        }
        assert!(inst.instances(MeshKind::Plane).is_empty());
        for p in 0..4 {
            let lin = style.linear(p);
            assert!(
                inst.instances(MeshKind::Cube)
                    .iter()
                    .any(|i| i.color[..3] == lin),
                "player {p} missing"
            );
        }
    }

    #[test]
    fn instances_stand_on_the_given_ground() {
        let mut inst = instancer(100, 64.0);
        let cam = camera(1.0, (32.0, 32.0), 64.0);
        inst.build(&Frustum::from_view_proj(cam.view_proj()), 0.0, |x, _| {
            x * 0.1
        });
        for i in inst.instances(MeshKind::Cube) {
            assert!((i.model[3][1] - i.model[3][0] * 0.1).abs() < 1e-5);
        }
    }

    #[test]
    fn tile_box_spans_the_relief_plus_the_tallest_shape() {
        let mut inst = Instancer::new(Style::embedded(), (64.0, 64.0));
        inst.set_height_range(-2.0, 20.0);
        inst.update_tick([motion(v2(40, 10), v2(41, 10), 0, 0)]);
        // The unit sits in tile (1, 0); its box covers both positions.
        let b = inst.tile_boxes[1];
        assert!(b.min.x < 40.0 && b.max.x > 41.0);
        assert_eq!(b.min.y, -2.0);
        assert!(b.max.y >= 20.0 + 1.4);
        // Interpolated positions stay inside the box.
        for a in [0.0, 0.25, 1.0] {
            let ([x, z], _) = inst.motions[0].at(a);
            assert!(b.min.x <= x && x <= b.max.x && b.min.z <= z && z <= b.max.z);
        }
    }
}
