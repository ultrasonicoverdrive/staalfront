//! The ground the camera and the picking work against (WP1.8): height at a
//! point and the first hit of a ray, for a flat world ([`FlatGround`]) or the
//! terrain ([`TerrainGround`]).
//!
//! The terrain version is exact: it intersects the ray with the very
//! triangles the full-detail mesh is drawn with (`terrain::cell_triangles`,
//! same diagonals), in `f64`. Coarser levels of detail deviate from that
//! surface by at most their geometric error, which is what the pixel budget
//! allows, so a click lands where the player sees the ground within that
//! tolerance.
//!
//! # Ray traversal
//! The ray is clipped to the map footprint and to the height range of the
//! whole map (rays from a high camera only start to matter once they dip
//! below the highest hill), then walked cell by cell front to back with a 2D
//! DDA (Amanatides and Woo). A cell is only tested against its two triangles
//! if the ray's height range over the cell overlaps the range of the cell's
//! corner heights. Walking in order means the first hit is the nearest one.
//! With the usual relief (a few metres) a camera ray crosses a handful of
//! cells; the cost grows with the horizontal distance the ray spends inside
//! the height range of the map, not with the map size.
//!
//! # Outside the map
//! The terrain is a finite square. Beyond its border the ground is the
//! horizontal plane at the **lowest height of the map**: a ray that leaves
//! the map without a hit lands on that plane, so the cursor still has a
//! ground point when it hovers beyond the border (the camera may look up to a
//! margin past the edge). A ray that points at or above the horizon, or that
//! starts under the ground, has no hit.

use std::fmt::Debug;

use glam::DVec3;
use sf_data::map::Map;

use crate::terrain::{cell_surface, cell_triangles, diagonal_nw_se};

/// Tolerance for "on the edge" in the traversal and the triangle test.
const EPS: f64 = 1e-9;

/// A ground to stand and pick on. Distances along rays are in units of the
/// direction passed in, so `origin + t * dir` is the hit point.
pub trait Ground: Debug + Send + Sync {
    /// Height of the ground at `(x, z)`.
    fn height(&self, x: f64, z: f64) -> f64;

    /// The nearest `t >= 0` where the ray `origin + t * dir` meets the
    /// ground, `None` if it never does (pointing at or above the horizon).
    fn raycast(&self, origin: DVec3, dir: DVec3) -> Option<f64>;

    /// The highest point of the ground.
    fn max_height(&self) -> f64;
}

/// The horizontal plane `y = height`: the flat world of WP1.5.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlatGround {
    pub y: f64,
}

impl Ground for FlatGround {
    fn height(&self, _x: f64, _z: f64) -> f64 {
        self.y
    }

    fn raycast(&self, origin: DVec3, dir: DVec3) -> Option<f64> {
        if dir.y >= -1e-9 {
            return None; // at or above the horizon
        }
        let t = (self.y - origin.y) / dir.y;
        (t >= 0.0).then_some(t)
    }

    fn max_height(&self) -> f64 {
        self.y
    }
}

/// The terrain of a map as a ground. Holds its own copy of the heights (a
/// `f32` per vertex), so it is independent of the `Map` and of the GPU.
#[derive(Clone, Debug)]
pub struct TerrainGround {
    width: usize,
    depth: usize,
    /// `(width + 1) * (depth + 1)` vertex heights, row-major (`z` rows).
    heights: Vec<f32>,
    min: f64,
    max: f64,
}

impl TerrainGround {
    pub fn new(map: &Map) -> Self {
        let (width, depth) = (map.width() as usize, map.height() as usize);
        let mut heights = Vec::with_capacity((width + 1) * (depth + 1));
        for z in 0..=depth {
            for x in 0..=width {
                heights.push(map.vertex_height(x as i64, z as i64));
            }
        }
        let (min, max) = heights
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &h| (lo.min(h), hi.max(h)));
        Self {
            width,
            depth,
            heights,
            min: f64::from(min),
            max: f64::from(max),
        }
    }

    /// Map size in cells, `(x, z)`.
    pub fn size(&self) -> (f64, f64) {
        (self.width as f64, self.depth as f64)
    }

    /// The lowest height of the map, which is also the height of the plane
    /// beyond its border.
    pub fn min_height(&self) -> f64 {
        self.min
    }

    /// True if `(x, z)` is over the map.
    pub fn contains(&self, x: f64, z: f64) -> bool {
        (0.0..=self.width as f64).contains(&x) && (0.0..=self.depth as f64).contains(&z)
    }

    fn vertex(&self, x: usize, z: usize) -> f64 {
        f64::from(self.heights[z * (self.width + 1) + x])
    }

    /// Corner heights `[nw, ne, sw, se]` of cell `(cx, cz)`.
    fn corners(&self, cx: usize, cz: usize) -> [f64; 4] {
        [
            self.vertex(cx, cz),
            self.vertex(cx + 1, cz),
            self.vertex(cx, cz + 1),
            self.vertex(cx + 1, cz + 1),
        ]
    }

    /// First hit with the terrain triangles only (no outside plane).
    fn raycast_cells(&self, o: DVec3, d: DVec3) -> Option<f64> {
        let (w, h) = (self.width as f64, self.depth as f64);
        // Clip to the footprint and the height range of the map.
        let (mut t0, mut t1) = (0.0_f64, f64::INFINITY);
        for (oa, da, lo, hi) in [
            (o.x, d.x, 0.0, w),
            (o.z, d.z, 0.0, h),
            (o.y, d.y, self.min - 1e-6, self.max + 1e-6),
        ] {
            if da.abs() < 1e-15 {
                if oa < lo - EPS || oa > hi + EPS {
                    return None;
                }
            } else {
                let (a, b) = ((lo - oa) / da, (hi - oa) / da);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if t0 > t1 {
            return None;
        }

        // Walk the cells from the entry point.
        let p = o + d * t0;
        let mut ix = (p.x.floor() as i64).clamp(0, self.width as i64 - 1);
        let mut iz = (p.z.floor() as i64).clamp(0, self.depth as i64 - 1);
        let sign = |v: f64| match v {
            v if v > 0.0 => 1_i64,
            v if v < 0.0 => -1,
            _ => 0,
        };
        let (step_x, step_z) = (sign(d.x), sign(d.z));
        let next = |i: i64, o: f64, d: f64| {
            if d > 0.0 {
                ((i + 1) as f64 - o) / d
            } else if d < 0.0 {
                (i as f64 - o) / d
            } else {
                f64::INFINITY
            }
        };
        let (mut tmax_x, mut tmax_z) = (next(ix, o.x, d.x), next(iz, o.z, d.z));
        let delta = |d: f64| {
            if d != 0.0 {
                1.0 / d.abs()
            } else {
                f64::INFINITY
            }
        };
        let (tdx, tdz) = (delta(d.x), delta(d.z));
        let mut t_cur = t0;
        loop {
            let t_next = tmax_x.min(tmax_z).min(t1);
            if let Some(t) = self.hit_cell(o, d, ix as usize, iz as usize, t_cur, t_next) {
                return Some(t);
            }
            if t_next >= t1 {
                return None;
            }
            if tmax_x < tmax_z {
                ix += step_x;
                tmax_x += tdx;
            } else {
                iz += step_z;
                tmax_z += tdz;
            }
            t_cur = t_next;
            if ix < 0 || iz < 0 || ix >= self.width as i64 || iz >= self.depth as i64 {
                return None;
            }
        }
    }

    /// The hit of the ray with the two triangles of one cell, if the ray's
    /// height range over `t_a..t_b` can reach them at all.
    fn hit_cell(
        &self,
        o: DVec3,
        d: DVec3,
        ix: usize,
        iz: usize,
        t_a: f64,
        t_b: f64,
    ) -> Option<f64> {
        let c = self.corners(ix, iz);
        let (ya, yb) = (o.y + d.y * t_a, o.y + d.y * t_b);
        let (lo, hi) = (ya.min(yb), ya.max(yb));
        let (cmin, cmax) = c
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        if hi < cmin - EPS || lo > cmax + EPS {
            return None;
        }
        let (x0, z0) = (ix as f64, iz as f64);
        let v = [
            DVec3::new(x0, c[0], z0),
            DVec3::new(x0 + 1.0, c[1], z0),
            DVec3::new(x0, c[2], z0 + 1.0),
            DVec3::new(x0 + 1.0, c[3], z0 + 1.0),
        ];
        cell_triangles(diagonal_nw_se(ix as u32, iz as u32))
            .iter()
            .filter_map(|t| ray_triangle(o, d, v[t[0]], v[t[1]], v[t[2]]))
            .min_by(f64::total_cmp)
    }
}

/// Moeller-Trumbore, two-sided, edges and corners count as hits. `t >= 0`.
fn ray_triangle(o: DVec3, d: DVec3, v0: DVec3, v1: DVec3, v2: DVec3) -> Option<f64> {
    let (e1, e2) = (v1 - v0, v2 - v0);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None; // parallel to the triangle
    }
    let inv = 1.0 / det;
    let tv = o - v0;
    let u = tv.dot(p) * inv;
    if !(-EPS..=1.0 + EPS).contains(&u) {
        return None;
    }
    let q = tv.cross(e1);
    let v = d.dot(q) * inv;
    if v < -EPS || u + v > 1.0 + EPS {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t >= 0.0).then_some(t)
}

impl Ground for TerrainGround {
    fn height(&self, x: f64, z: f64) -> f64 {
        if !self.contains(x, z) {
            return self.min;
        }
        let cx = (x.floor() as usize).min(self.width - 1);
        let cz = (z.floor() as usize).min(self.depth - 1);
        cell_surface(
            self.corners(cx, cz),
            x - cx as f64,
            z - cz as f64,
            diagonal_nw_se(cx as u32, cz as u32),
        )
    }

    fn raycast(&self, origin: DVec3, dir: DVec3) -> Option<f64> {
        let len = dir.length();
        if len == 0.0 || !len.is_finite() {
            return None;
        }
        let d = dir / len;
        let mut best = self.raycast_cells(origin, d);
        // Beyond the border the ground is the plane at the lowest height.
        if d.y < -1e-12 {
            let t = (self.min - origin.y) / d.y;
            if t >= 0.0 {
                let p = origin + d * t;
                if !self.contains(p.x, p.z) {
                    best = Some(best.map_or(t, |b| b.min(t)));
                }
            }
        }
        best.map(|t| t / len)
    }

    fn max_height(&self) -> f64 {
        self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::surface_height;
    use sf_data::map::generate::generate;

    /// A deterministic stream of numbers in `0..1`.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 11) as f64 / (1_u64 << 53) as f64
        }

        fn range(&mut self, lo: f64, hi: f64) -> f64 {
            lo + (hi - lo) * self.next()
        }
    }

    /// The nearest hit found by testing every triangle of the map, plus the
    /// outside plane: the slow, obviously right version.
    fn brute_force(g: &TerrainGround, o: DVec3, dir: DVec3) -> Option<f64> {
        let len = dir.length();
        let d = dir / len;
        let mut best: Option<f64> = None;
        for cz in 0..g.depth {
            for cx in 0..g.width {
                let c = g.corners(cx, cz);
                let (x0, z0) = (cx as f64, cz as f64);
                let v = [
                    DVec3::new(x0, c[0], z0),
                    DVec3::new(x0 + 1.0, c[1], z0),
                    DVec3::new(x0, c[2], z0 + 1.0),
                    DVec3::new(x0 + 1.0, c[3], z0 + 1.0),
                ];
                for t in cell_triangles(diagonal_nw_se(cx as u32, cz as u32)) {
                    if let Some(hit) = ray_triangle(o, d, v[t[0]], v[t[1]], v[t[2]]) {
                        best = Some(best.map_or(hit, |b| b.min(hit)));
                    }
                }
            }
        }
        if d.y < -1e-12 {
            let t = (g.min - o.y) / d.y;
            if t >= 0.0 {
                let p = o + d * t;
                if !g.contains(p.x, p.z) {
                    best = Some(best.map_or(t, |b| b.min(t)));
                }
            }
        }
        best.map(|t| t / len)
    }

    fn hilly() -> TerrainGround {
        TerrainGround::new(&generate(80, 64, 3))
    }

    #[test]
    fn flat_ground_hits_the_plane_below_and_nothing_above() {
        let g = FlatGround::default();
        let o = DVec3::new(3.0, 10.0, 4.0);
        let t = g.raycast(o, DVec3::new(0.0, -2.0, 1.0)).unwrap();
        assert!((o + DVec3::new(0.0, -2.0, 1.0) * t).y.abs() < 1e-12);
        assert!(g.raycast(o, DVec3::new(1.0, 0.0, 0.0)).is_none());
        assert!(g.raycast(o, DVec3::new(1.0, 1.0, 0.0)).is_none());
        // Below the plane, looking down: behind the origin, no hit.
        assert!(
            g.raycast(DVec3::new(0.0, -1.0, 0.0), DVec3::NEG_Y)
                .is_none()
        );
    }

    #[test]
    fn height_is_the_height_of_the_drawn_mesh() {
        let map = generate(70, 40, 9);
        let g = TerrainGround::new(&map);
        let mut rng = Lcg(1);
        for _ in 0..2000 {
            let (x, z) = (rng.range(0.0, 70.0), rng.range(0.0, 40.0));
            assert!((g.height(x, z) - surface_height(&map, x, z)).abs() < 1e-9);
        }
        // Outside the map: the plane at the lowest height.
        assert_eq!(g.height(-5.0, 10.0), g.min_height());
        assert_eq!(g.height(10.0, 400.0), g.min_height());
    }

    #[test]
    fn rays_agree_with_testing_every_triangle() {
        let g = hilly();
        let mut rng = Lcg(42);
        let mut hits = 0;
        let mut misses = 0;
        for i in 0..3000 {
            // Origins over, beside and well outside the map; mostly downward.
            let o = DVec3::new(
                rng.range(-40.0, 120.0),
                rng.range(0.0, 60.0),
                rng.range(-40.0, 100.0),
            );
            let dir = DVec3::new(
                rng.range(-1.0, 1.0),
                if i % 7 == 0 {
                    rng.range(-0.2, 0.3)
                } else {
                    rng.range(-1.0, -0.05)
                },
                rng.range(-1.0, 1.0),
            );
            let fast = g.raycast(o, dir);
            let slow = brute_force(&g, o, dir);
            match (fast, slow) {
                (Some(a), Some(b)) => {
                    assert!(
                        (a - b).abs() < 1e-6 * (1.0 + b.abs()),
                        "ray {i}: {a} vs {b}"
                    );
                    hits += 1;
                }
                (None, None) => misses += 1,
                other => panic!("ray {i} from {o} along {dir}: {other:?}"),
            }
        }
        assert!(hits > 1500 && misses > 50, "{hits} hits, {misses} misses");
    }

    #[test]
    fn the_hit_point_lies_on_the_surface() {
        let g = hilly();
        let mut rng = Lcg(7);
        for _ in 0..1000 {
            let o = DVec3::new(
                rng.range(5.0, 75.0),
                rng.range(20.0, 80.0),
                rng.range(5.0, 60.0),
            );
            let dir = DVec3::new(rng.range(-0.6, 0.6), -1.0, rng.range(-0.6, 0.6));
            let t = g.raycast(o, dir).expect("a downward ray over the map hits");
            let p = o + dir * t;
            if g.contains(p.x, p.z) {
                assert!((p.y - g.height(p.x, p.z)).abs() < 1e-7, "{p}");
            }
        }
    }

    #[test]
    fn a_hill_hides_what_is_behind_it() {
        // A ridge across the middle of a flat map.
        let (w, h) = (16_usize, 16_usize);
        let mut heights = vec![0_u16; (w + 1) * (h + 1)];
        for z in 0..=h {
            for x in 7..=9 {
                heights[z * (w + 1) + x] = 65535;
            }
        }
        let map = Map::from_parts(
            w as u32,
            h as u32,
            10.0,
            vec!["a".into()],
            heights,
            vec![0; w * h],
        )
        .unwrap();
        let g = TerrainGround::new(&map);
        // From the west, 3 m up, looking slightly down: the ridge (10 m) is hit first.
        let o = DVec3::new(0.5, 3.0, 8.0);
        let dir = DVec3::new(1.0, -0.1, 0.0);
        let t = g.raycast(o, dir).unwrap();
        let p = o + dir * t;
        assert!(p.x < 7.5 && p.y > 2.0, "hit {p}");
        // A flat plane would have been hit far behind the ridge.
        assert!(FlatGround::default().raycast(o, dir).unwrap() * dir.x > 20.0);
    }

    #[test]
    fn rays_that_leave_the_map_land_on_the_plane_at_the_lowest_height() {
        let g = hilly();
        let o = DVec3::new(100.0, 30.0, 30.0); // east of the 80 m wide map
        let t = g.raycast(o, DVec3::new(0.2, -1.0, 0.0)).unwrap();
        let p = o + DVec3::new(0.2, -1.0, 0.0) * t;
        assert!((p.y - g.min_height()).abs() < 1e-9);
        assert!(!g.contains(p.x, p.z));
        // Horizontal and upward rays over the map do not hit.
        assert!(g.raycast(DVec3::new(40.0, 50.0, 30.0), DVec3::X).is_none());
        assert!(g.raycast(DVec3::new(40.0, 50.0, 30.0), DVec3::Y).is_none());
    }

    #[test]
    fn rays_through_vertices_and_along_cell_edges_do_not_slip_through() {
        // Integer coordinates and axis-aligned directions: the ray runs exactly
        // along cell borders and through vertices.
        let g = hilly();
        for (ox, oz) in [
            (10.0, 10.0),
            (20.0, 33.0),
            (41.0, 7.0),
            (0.0, 0.0),
            (80.0, 64.0),
        ] {
            let o = DVec3::new(ox, 40.0, oz);
            for dir in [
                DVec3::new(0.0, -1.0, 0.0),
                DVec3::new(0.25, -1.0, 0.0),
                DVec3::new(0.0, -1.0, 0.25),
                DVec3::new(0.5, -1.0, 0.5),
                DVec3::new(-0.5, -1.0, -0.5),
            ] {
                let fast = g.raycast(o, dir);
                let slow = brute_force(&g, o, dir);
                assert_eq!(fast.is_some(), slow.is_some(), "{o} {dir}");
                if let (Some(a), Some(b)) = (fast, slow) {
                    assert!((a - b).abs() < 1e-6, "{o} {dir}: {a} vs {b}");
                }
            }
        }
    }
}
