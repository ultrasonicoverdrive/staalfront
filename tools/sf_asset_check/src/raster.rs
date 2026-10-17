//! The top-down view: which share of the visible area is `team` colour.
//!
//! The triangles are projected onto the ground plane (`x`, `z`) and rasterised
//! on a square pixel grid; per pixel the triangle with the highest `y` there
//! is the one a camera looking straight down sees. The share is team pixels
//! over covered pixels. The grid is [`RESOLUTION`] pixels along the longer
//! side of the footprint, so the share is accurate to about a percent for
//! models of this size.

use crate::geom::Tri;

/// Pixels along the longer side of the footprint.
pub const RESOLUTION: usize = 512;

/// Pixels covered by the model and, of those, the team-coloured ones.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TopDown {
    pub covered: u32,
    pub team: u32,
}

impl TopDown {
    /// Team pixels over covered pixels; `None` if nothing is covered.
    pub fn team_share(&self) -> Option<f64> {
        (self.covered > 0).then(|| f64::from(self.team) / f64::from(self.covered))
    }
}

/// Rasterise `tris` seen from above (`+Y` towards the camera).
pub fn top_down(tris: &[Tri]) -> TopDown {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for t in tris {
        for p in &t.v {
            let q = [f64::from(p.x), f64::from(p.z)];
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    let longest = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if tris.is_empty() || longest <= 0.0 {
        return TopDown {
            covered: 0,
            team: 0,
        };
    }
    let pixel = longest / RESOLUTION as f64;
    let width = ((hi[0] - lo[0]) / pixel).ceil() as usize + 1;
    let depth_rows = ((hi[1] - lo[1]) / pixel).ceil() as usize + 1;
    // Highest y seen per pixel, and whether that triangle is team.
    let mut top = vec![f64::NEG_INFINITY; width * depth_rows];
    let mut is_team = vec![false; width * depth_rows];
    for t in tris {
        let a = [
            f64::from(t.v[0].x),
            f64::from(t.v[0].z),
            f64::from(t.v[0].y),
        ];
        let b = [
            f64::from(t.v[1].x),
            f64::from(t.v[1].z),
            f64::from(t.v[1].y),
        ];
        let c = [
            f64::from(t.v[2].x),
            f64::from(t.v[2].z),
            f64::from(t.v[2].y),
        ];
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-12 {
            continue; // seen edge-on from above (a wall)
        }
        let min_x = a[0].min(b[0]).min(c[0]);
        let max_x = a[0].max(b[0]).max(c[0]);
        let min_z = a[1].min(b[1]).min(c[1]);
        let max_z = a[1].max(b[1]).max(c[1]);
        let px0 = (((min_x - lo[0]) / pixel - 0.5).floor().max(0.0)) as usize;
        let px1 = ((((max_x - lo[0]) / pixel - 0.5).ceil()).max(0.0) as usize).min(width - 1);
        let pz0 = (((min_z - lo[1]) / pixel - 0.5).floor().max(0.0)) as usize;
        let pz1 = ((((max_z - lo[1]) / pixel - 0.5).ceil()).max(0.0) as usize).min(depth_rows - 1);
        for pz in pz0..=pz1 {
            for px in px0..=px1 {
                let x = lo[0] + (px as f64 + 0.5) * pixel;
                let z = lo[1] + (pz as f64 + 0.5) * pixel;
                // Barycentric weights of the pixel centre.
                let w0 = ((b[0] - x) * (c[1] - z) - (b[1] - z) * (c[0] - x)) / area;
                let w1 = ((c[0] - x) * (a[1] - z) - (c[1] - z) * (a[0] - x)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                    continue;
                }
                let y = w0 * a[2] + w1 * b[2] + w2 * c[2];
                let at = pz * width + px;
                if y > top[at] {
                    top[at] = y;
                    is_team[at] = t.team;
                }
            }
        }
    }
    let mut out = TopDown {
        covered: 0,
        team: 0,
    };
    for (y, team) in top.iter().zip(&is_team) {
        if *y > f64::NEG_INFINITY {
            out.covered += 1;
            out.team += u32::from(*team);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    /// A rectangle `x0..x1`, `z0..z1` at height `y` as two triangles.
    fn rect(x0: f32, x1: f32, z0: f32, z1: f32, y: f32, team: bool) -> [Tri; 2] {
        let p = |x, z| Vec3::new(x, y, z);
        [
            Tri {
                v: [p(x0, z0), p(x1, z1), p(x1, z0)],
                team,
            },
            Tri {
                v: [p(x0, z0), p(x0, z1), p(x1, z1)],
                team,
            },
        ]
    }

    fn share(tris: &[Tri]) -> f64 {
        top_down(tris).team_share().unwrap()
    }

    #[test]
    fn a_team_strip_over_a_third_of_the_floor_is_a_third() {
        let mut tris = rect(0.0, 3.0, 0.0, 1.0, 0.0, false).to_vec();
        tris.extend(rect(0.0, 1.0, 0.0, 1.0, 0.5, true));
        let s = share(&tris);
        assert!((s - 1.0 / 3.0).abs() < 0.01, "{s}");
    }

    #[test]
    fn only_the_topmost_triangle_counts() {
        // Team floor, hidden by a non-team roof over all of it: 0 %.
        let mut tris = rect(0.0, 2.0, 0.0, 2.0, 0.0, true).to_vec();
        tris.extend(rect(0.0, 2.0, 0.0, 2.0, 1.0, false));
        assert_eq!(share(&tris), 0.0);
        // The other way round: all team.
        let mut tris = rect(0.0, 2.0, 0.0, 2.0, 0.0, false).to_vec();
        tris.extend(rect(0.0, 2.0, 0.0, 2.0, 1.0, true));
        assert_eq!(share(&tris), 1.0);
    }

    #[test]
    fn walls_seen_edge_on_add_no_area() {
        let wall = [Tri {
            v: [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            ],
            team: true,
        }];
        assert_eq!(top_down(&wall).covered, 0);
        assert_eq!(top_down(&wall).team_share(), None);
        assert_eq!(top_down(&[]).team_share(), None);
    }

    #[test]
    fn the_share_does_not_depend_on_the_size_of_the_model() {
        for scale in [0.1_f32, 1.0, 40.0] {
            let mut tris = rect(0.0, 4.0 * scale, 0.0, scale, 0.0, false).to_vec();
            tris.extend(rect(0.0, scale, 0.0, scale, 0.1, true));
            let s = share(&tris);
            assert!((s - 0.25).abs() < 0.01, "scale {scale}: {s}");
        }
    }
}
