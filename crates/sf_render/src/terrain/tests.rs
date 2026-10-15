use super::*;
use sf_data::map::generate::generate;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
}

/// White-noise terrain, `scale` metres at most: the worst case for any
/// simplification (every vertex independent), so both diagonals and the cell
/// centres matter.
fn rough_map(width: u32, height: u32, scale: f32, seed: u64) -> Map {
    let mut rng = Lcg(seed);
    let heights = (0..(width + 1) * (height + 1))
        .map(|_| (rng.next() * 65535.0) as u16)
        .collect();
    Map::from_parts(
        width,
        height,
        scale,
        vec!["a".into()],
        heights,
        vec![0; (width * height) as usize],
    )
    .expect("valid rough map")
}

/// Height of a chunk's level at a world position, read from its own
/// triangles (the two triangles of the coarse cell containing the point).
fn lod_height(c: &TerrainChunk, level: u8, x: f64, z: f64) -> f64 {
    let m = c.level(level);
    let step = f64::from(m.step);
    let cw = m.grid.0 - 1;
    let ch = m.grid.1 - 1;
    let i = (((x - f64::from(c.origin.0)) / step).floor() as u32).min(cw - 1);
    let j = (((z - f64::from(c.origin.1)) / step).floor() as u32).min(ch - 1);
    let base = ((j * cw + i) * 6) as usize;
    for tri in m.surface[base..base + 6].chunks(3) {
        let p = |k: usize| {
            let v = m.vertices[tri[k] as usize].position;
            (f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
        };
        let (a, b, cc) = (p(0), p(1), p(2));
        let det = (b.2 - cc.2) * (a.0 - cc.0) + (cc.0 - b.0) * (a.2 - cc.2);
        let l1 = ((b.2 - cc.2) * (x - cc.0) + (cc.0 - b.0) * (z - cc.2)) / det;
        let l2 = ((cc.2 - a.2) * (x - cc.0) + (a.0 - cc.0) * (z - cc.2)) / det;
        let l3 = 1.0 - l1 - l2;
        if l1 >= -1e-9 && l2 >= -1e-9 && l3 >= -1e-9 {
            return l1 * a.1 + l2 * b.1 + l3 * cc.1;
        }
    }
    panic!(
        "({x}, {z}) is in no triangle of chunk {:?} level {level}",
        c.chunk
    );
}

fn normal_of(m: &LodMesh, tri: &[u32]) -> Vec3 {
    let p = |i: u32| Vec3::from(m.vertices[i as usize].position);
    (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]))
}

// ---------------------------------------------------------------------------
// Chunks and levels
// ---------------------------------------------------------------------------

fn check_chunk_basics(map: &Map, mesh: &TerrainMesh) {
    for c in &mesh.chunks {
        for m in &c.levels {
            let (gw, gh) = (c.cells.0 / m.step + 1, c.cells.1 / m.step + 1);
            assert_eq!(m.grid, (gw, gh));
            let surface_vertices = (gw * gh) as usize;
            assert!(m.vertices.len() >= surface_vertices);
            assert_eq!(
                m.surface.len(),
                (c.cells.0 / m.step * (c.cells.1 / m.step) * 6) as usize
            );
            assert!(m.surface.iter().all(|&i| (i as usize) < surface_vertices));
            assert!(m.skirt.iter().all(|&i| (i as usize) < m.vertices.len()));
            for v in &m.vertices {
                let len = Vec3::from(v.normal).length();
                assert!((len - 1.0).abs() < 1e-5, "normal length {len}");
                assert!(v.normal[1] > 0.0);
                assert_eq!(v.lod, u32::from(m.level));
                assert!(
                    v.position[1] >= c.aabb.min.y - SKIRT_MARGIN - 1e-3
                        && v.position[1] <= c.aabb.max.y,
                    "vertex outside the chunk box"
                );
            }
            // Every surface triangle faces up (counter-clockwise from above).
            for t in m.surface.chunks(3) {
                assert!(
                    normal_of(m, t).y > 0.0,
                    "downward triangle in chunk {:?}",
                    c.chunk
                );
            }
        }
    }
    let cells: u32 = mesh.chunks.iter().map(|c| c.cells.0 * c.cells.1).sum();
    assert_eq!(cells, map.width() * map.height());
}

#[test]
fn a_64x64_map_makes_four_chunks_with_six_levels() {
    let map = generate(64, 64, 1);
    let mesh = build_mesh(&map);
    assert_eq!((mesh.chunks_x, mesh.chunks_z), (2, 2));
    assert_eq!(mesh.chunks.len(), 4);
    for c in &mesh.chunks {
        assert_eq!(c.cells, (32, 32));
        assert_eq!(c.levels.len(), LOD_LEVELS);
        let full = c.level(0);
        assert_eq!(full.grid, (33, 33));
        assert_eq!(full.surface.len(), 32 * 32 * 6);
        assert_eq!(full.error, 0.0);
    }
    check_chunk_basics(&map, &mesh);
}

#[test]
fn edge_chunks_of_odd_sized_maps_are_smaller_and_have_fewer_levels() {
    let map = generate(70, 40, 2);
    let mesh = build_mesh(&map);
    assert_eq!((mesh.chunks_x, mesh.chunks_z), (3, 2));
    let last = mesh.chunks.last().unwrap();
    assert_eq!(last.cells, (6, 8));
    // Steps must divide both sides: 1 and 2.
    assert_eq!(last.levels.len(), 2);
    assert_eq!(mesh.chunks[0].levels.len(), LOD_LEVELS);
    // 6 x 32 (east edge, top row): gcd 2.
    assert_eq!(mesh.chunks[2].cells, (6, 32));
    assert_eq!(mesh.chunks[2].levels.len(), 2);
    // 32 x 8 (bottom row): gcd 8 -> steps up to 8.
    assert_eq!(mesh.chunks[3].cells, (32, 8));
    assert_eq!(mesh.chunks[3].levels.len(), 4);
    check_chunk_basics(&map, &mesh);
    // A 1-cell chunk only has full detail.
    let tiny = build_mesh(&generate(33, 33, 4));
    assert_eq!(tiny.chunks.last().unwrap().cells, (1, 1));
    assert_eq!(tiny.chunks.last().unwrap().levels.len(), 1);
}

/// Shared border vertices are identical in neighbouring chunks (position
/// and normal, bit for bit), and every vertex is the map's own height.
#[test]
fn full_detail_chunk_borders_are_bit_identical() {
    let map = generate(96, 64, 5);
    let mesh = build_mesh(&map);
    let chunk = |cx, cz| &mesh.chunks[mesh.chunk_index(cx, cz)];
    let vert = |c: &TerrainChunk, x: u32, z: u32| *c.level(0).grid_vertex(x, z);
    let mut compared = 0;
    for cz in 0..mesh.chunks_z {
        for cx in 0..mesh.chunks_x {
            let c = chunk(cx, cz);
            if cx + 1 < mesh.chunks_x {
                let e = chunk(cx + 1, cz);
                for z in 0..=c.cells.1 {
                    assert_eq!(vert(c, c.cells.0, z), vert(e, 0, z));
                    compared += 1;
                }
            }
            if cz + 1 < mesh.chunks_z {
                let s = chunk(cx, cz + 1);
                for x in 0..=c.cells.0 {
                    assert_eq!(vert(c, x, c.cells.1), vert(s, x, 0));
                    compared += 1;
                }
            }
        }
    }
    assert!(compared > 100);
}

#[test]
fn every_level_sits_on_the_global_lattice_with_exact_heights() {
    let map = generate(96, 64, 6);
    let mesh = build_mesh(&map);
    for c in &mesh.chunks {
        for m in &c.levels {
            for j in 0..m.grid.1 {
                for i in 0..m.grid.0 {
                    let v = m.grid_vertex(i, j);
                    let (gx, gz) = (c.origin.0 + i * m.step, c.origin.1 + j * m.step);
                    assert_eq!(v.position[0], gx as f32);
                    assert_eq!(v.position[2], gz as f32);
                    assert_eq!(
                        v.position[1],
                        map.vertex_height(i64::from(gx), i64::from(gz))
                    );
                }
            }
        }
    }
}

#[test]
fn each_level_has_a_quarter_of_the_triangles() {
    let mesh = build_mesh(&generate(64, 64, 1));
    for c in &mesh.chunks {
        for (l, m) in c.levels.iter().enumerate() {
            let quads = 32 >> l;
            assert_eq!(m.surface_triangles(), 2 * quads * quads, "level {l}");
        }
    }
    // Whole map: skirts included, strictly fewer triangles per level.
    let counts: Vec<u64> = (0..LOD_LEVELS as u8)
        .map(|l| mesh.triangles_at(l))
        .collect();
    println!("triangles per level (64x64, with skirts): {counts:?}");
    assert!(counts.windows(2).all(|w| w[0] > w[1]), "{counts:?}");
}

#[test]
fn surface_height_is_the_full_detail_mesh() {
    let map = generate(64, 64, 8);
    let mesh = build_mesh(&map);
    let mut rng = Lcg(5);
    for _ in 0..3000 {
        let (x, z) = (rng.next() * 64.0, rng.next() * 64.0);
        let c = &mesh.chunks[mesh.chunk_index((x as u32 / 32).min(1), (z as u32 / 32).min(1))];
        let from_mesh = lod_height(c, 0, x, z);
        assert!(
            (from_mesh - surface_height(&map, x, z)).abs() < 1e-5,
            "({x}, {z})"
        );
    }
}

// ---------------------------------------------------------------------------
// Geometric error
// ---------------------------------------------------------------------------

/// Largest `|full - level|` over points of a chunk on a grid of `spacing`
/// metres plus random points.
fn sampled_error(map: &Map, c: &TerrainChunk, level: u8, spacing: f64, rng: &mut Lcg) -> f64 {
    let mut worst = 0.0_f64;
    let mut check = |x: f64, z: f64| {
        let d = (surface_height(map, x, z) - lod_height(c, level, x, z)).abs();
        worst = worst.max(d);
    };
    let (ox, oz) = (f64::from(c.origin.0), f64::from(c.origin.1));
    let (w, h) = (f64::from(c.cells.0), f64::from(c.cells.1));
    let (nx, nz) = ((w / spacing) as u32, (h / spacing) as u32);
    for j in 0..=nz {
        for i in 0..=nx {
            check(ox + f64::from(i) * spacing, oz + f64::from(j) * spacing);
        }
    }
    for _ in 0..2000 {
        check(ox + rng.next() * w, oz + rng.next() * h);
    }
    worst
}

#[test]
fn the_stated_error_bounds_the_real_error_and_is_tight() {
    let mut rng = Lcg(11);
    for map in [
        generate(64, 64, 1),
        rough_map(64, 64, 12.0, 3),
        // Odd chunk sizes: the east and south chunks are 6 and 8 cells.
        rough_map(38, 40, 6.0, 4),
    ] {
        let mesh = build_mesh(&map);
        for c in &mesh.chunks {
            for level in 1..=c.max_level() {
                let m = c.level(level);
                // 0.5 m reaches the lattice points and cell centres, which is
                // where the error peaks.
                let sampled = sampled_error(&map, c, level, 0.5, &mut rng);
                let stated = f64::from(m.error);
                assert!(
                    sampled <= stated + 1e-6,
                    "chunk {:?} level {level}: sampled {sampled} exceeds stated {stated}",
                    c.chunk
                );
                if level == 1 {
                    // No running maximum applies to the first level: the
                    // stated error is the measured one.
                    assert!(
                        stated - sampled < 1e-3,
                        "chunk {:?} level 1: stated {stated} is loose against sampled {sampled}",
                        c.chunk
                    );
                }
            }
        }
    }
}

/// The error of one coarse cell is exact: it equals the largest difference
/// found on a fine grid of sample points (which includes the lattice points
/// and the cell centres where it can peak), for both diagonals, odd and even
/// parity and random heights.
#[test]
fn coarse_cell_error_is_exact_for_random_cells() {
    let mut rng = Lcg(77);
    let mut centre_decided = 0;
    for &s in &[2_usize, 4, 8] {
        for case in 0..300 {
            let w = s + 1;
            let full: Vec<f64> = (0..w * w).map(|_| rng.next() * 10.0).collect();
            let origin_even = case % 2 == 0;
            let corners = [full[0], full[s], full[s * w], full[s * w + s]];
            for nw_se in [true, false] {
                let stated = coarse_cell_error(&full, s, nw_se, origin_even);
                // Fine grid: eighths of a full-detail cell.
                let mut sampled = 0.0_f64;
                let mut lattice_only = 0.0_f64;
                let n = 8 * s;
                for j in 0..=n {
                    for i in 0..=n {
                        let (x, z) = (i as f64 / 8.0, j as f64 / 8.0);
                        let (a, b) = (
                            (x.floor() as usize).min(s - 1),
                            (z.floor() as usize).min(s - 1),
                        );
                        let c = [
                            full[b * w + a],
                            full[b * w + a + 1],
                            full[(b + 1) * w + a],
                            full[(b + 1) * w + a + 1],
                        ];
                        let fine_nw_se = origin_even == (a + b).is_multiple_of(2);
                        let fine = cell_surface(c, x - a as f64, z - b as f64, fine_nw_se);
                        let lod = cell_surface(corners, x / s as f64, z / s as f64, nw_se);
                        let d = (fine - lod).abs();
                        sampled = sampled.max(d);
                        if i % 8 == 0 && j % 8 == 0 {
                            lattice_only = lattice_only.max(d);
                        }
                    }
                }
                assert!(
                    (stated - sampled).abs() < 1e-9,
                    "s={s} case {case} nw_se={nw_se}: stated {stated} vs sampled {sampled}"
                );
                if sampled > lattice_only + 1e-9 {
                    centre_decided += 1;
                }
            }
        }
    }
    // The cell centres do decide the error in a fair share of random cells, so
    // leaving them out would be caught here.
    assert!(centre_decided > 50, "{centre_decided}");
}

#[test]
fn errors_never_decrease_with_the_level() {
    for map in [generate(96, 64, 2), rough_map(64, 64, 9.0, 8)] {
        for c in &build_mesh(&map).chunks {
            assert_eq!(c.level(0).error, 0.0);
            for w in c.levels.windows(2) {
                assert!(w[0].error <= w[1].error, "chunk {:?}", c.chunk);
            }
        }
    }
}

#[test]
fn flat_ground_costs_nothing_to_simplify_and_hills_do() {
    let flat = Map::from_parts(
        64,
        64,
        5.0,
        vec!["a".into()],
        vec![20_000; 65 * 65],
        vec![0; 64 * 64],
    )
    .unwrap();
    for c in &build_mesh(&flat).chunks {
        assert!(c.levels.iter().all(|m| m.error < 1e-4), "{:?}", c.chunk);
    }
    let hills = build_mesh(&generate(64, 64, 1));
    assert!(hills.chunks.iter().all(|c| c.level(MAX_LEVEL).error > 0.2));
}

#[test]
fn a_coarse_cell_reports_the_exact_maximum_error() {
    // One tall corner: the two diagonals fit it differently.
    let mut heights = vec![0_u16; 5 * 5];
    heights[2 * 5 + 2] = 65535; // the middle vertex of a 4x4 map
    let map = Map::from_parts(4, 4, 8.0, vec!["a".into()], heights, vec![0; 16]).unwrap();
    let mesh = build_mesh(&map);
    // Level 2: one quad for the whole 4x4 chunk; the spike in the middle is
    // missed by it, so the error is at least the spike's height on the diagonal.
    let m = mesh.chunks[0].level(2);
    assert_eq!(m.surface.len(), 6);
    let sampled = sampled_error(&map, &mesh.chunks[0], 2, 0.25, &mut Lcg(1));
    assert!(
        (f64::from(m.error) - sampled).abs() < 1e-3,
        "{} vs {sampled}",
        m.error
    );
    assert!(sampled > 7.9, "{sampled}");
}

// ---------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------

/// The surface of chunk `c` at `level` along `side` at the edge position `t`
/// (`x` for north and south, `z` for east and west).
fn edge_height(c: &TerrainChunk, level: u8, side: Side, t: f64) -> f64 {
    let (x, z) = match side {
        Side::North => (t, f64::from(c.origin.1)),
        Side::South => (t, f64::from(c.origin.1 + c.cells.1)),
        Side::West => (f64::from(c.origin.0), t),
        Side::East => (f64::from(c.origin.0 + c.cells.0), t),
    };
    lod_height(c, level, x, z)
}

/// The largest height, over the shared edge of `a` (at `la`, on `side`) and
/// `b` (at `lb`), of the part of the gap between the two surfaces that
/// neither skirt covers. A wall hangs from a surface down to its bottom, so
/// the higher surface's wall must reach the lower surface. `walls = false`
/// measures the raw gap.
fn uncovered_gap(
    a: &TerrainChunk,
    la: u8,
    side: Side,
    b: &TerrainChunk,
    lb: u8,
    walls: bool,
) -> f64 {
    let (lo, hi) = match side {
        Side::East | Side::West => (f64::from(a.origin.1), f64::from(a.origin.1 + a.cells.1)),
        _ => (f64::from(a.origin.0), f64::from(a.origin.0 + a.cells.0)),
    };
    let wall = |c: &TerrainChunk, level: u8, side: Side, h: f64| {
        if walls {
            c.level(level).skirt_bottom[side.index()].map_or(h, f64::from)
        } else {
            h
        }
    };
    let mut worst = 0.0_f64;
    let n = 4 * (hi - lo) as u32;
    for k in 0..=n {
        let t = lo + (hi - lo) * f64::from(k) / f64::from(n);
        let (ha, hb) = (
            edge_height(a, la, side, t),
            edge_height(b, lb, side.opposite(), t),
        );
        let (wa, wb) = (wall(a, la, side, ha), wall(b, lb, side.opposite(), hb));
        // The gap is [min(ha, hb), max(ha, hb)]; the higher side's wall covers
        // down to its bottom.
        let uncovered = if ha >= hb {
            (wa - hb).max(0.0)
        } else {
            (wb - ha).max(0.0)
        };
        worst = worst.max(uncovered);
    }
    worst
}

/// For every pair of neighbouring chunks and every combination of levels:
/// `(largest uncovered gap, largest raw gap between the two surfaces)`.
fn seam_report(mesh: &TerrainMesh) -> (f64, f64) {
    let (mut uncovered, mut raw) = (0.0_f64, 0.0_f64);
    for cz in 0..mesh.chunks_z {
        for cx in 0..mesh.chunks_x {
            let a = &mesh.chunks[mesh.chunk_index(cx, cz)];
            let mut pairs = Vec::new();
            if cx + 1 < mesh.chunks_x {
                pairs.push((Side::East, &mesh.chunks[mesh.chunk_index(cx + 1, cz)]));
            }
            if cz + 1 < mesh.chunks_z {
                pairs.push((Side::South, &mesh.chunks[mesh.chunk_index(cx, cz + 1)]));
            }
            for (side, b) in pairs {
                for la in 0..=a.max_level() {
                    for lb in 0..=b.max_level() {
                        uncovered = uncovered.max(uncovered_gap(a, la, side, b, lb, true));
                        raw = raw.max(uncovered_gap(a, la, side, b, lb, false));
                    }
                }
            }
        }
    }
    (uncovered, raw)
}

/// THE seam test: whatever the levels of two neighbouring chunks, the skirts
/// leave no gap. The raw gap is measured too, so the test has something to
/// close.
#[test]
fn skirts_close_every_gap_between_every_pair_of_levels() {
    for map in [rough_map(96, 64, 30.0, 21), generate(96, 64, 12)] {
        let mesh = build_mesh(&map);
        let (uncovered, raw) = seam_report(&mesh);
        println!("largest raw gap {raw:.3} m, uncovered with skirts {uncovered:.6} m");
        assert!(raw > 0.3, "the map has no gaps worth closing: {raw}");
        assert_eq!(uncovered, 0.0);
    }
}

/// The probe: the same measurement on a mesh built without skirts must find
/// gaps, otherwise the test above could not fail.
#[test]
fn without_skirts_the_seam_test_finds_the_cracks() {
    let map = rough_map(96, 64, 30.0, 21);
    let mesh = build_mesh_with(&map, MeshOptions { skirts: false });
    assert!(mesh.chunks.iter().all(|c| {
        c.levels
            .iter()
            .all(|m| m.skirt.is_empty() && m.skirt_bottom == [None; 4])
    }));
    let (uncovered, raw) = seam_report(&mesh);
    println!("without skirts: uncovered {uncovered:.3} m (raw {raw:.3} m)");
    assert!(uncovered > 0.3 && (uncovered - raw).abs() < 1e-9);
}

/// Equal levels meet along identical edges: no gap even without skirts.
#[test]
fn equal_levels_meet_exactly() {
    let map = rough_map(96, 64, 30.0, 22);
    let mesh = build_mesh_with(&map, MeshOptions { skirts: false });
    for cz in 0..mesh.chunks_z {
        for cx in 0..mesh.chunks_x - 1 {
            let (a, b) = (
                &mesh.chunks[mesh.chunk_index(cx, cz)],
                &mesh.chunks[mesh.chunk_index(cx + 1, cz)],
            );
            for l in 0..=a.max_level().min(b.max_level()) {
                assert_eq!(
                    uncovered_gap(a, l, Side::East, b, l, true),
                    0.0,
                    "level {l}"
                );
            }
        }
    }
}

#[test]
fn skirts_hang_from_the_edge_vertices_and_face_outward() {
    let map = generate(96, 64, 12);
    let mesh = build_mesh(&map);
    for cz in 0..mesh.chunks_z {
        for cx in 0..mesh.chunks_x {
            let c = &mesh.chunks[mesh.chunk_index(cx, cz)];
            let has = [
                cz > 0,
                cx + 1 < mesh.chunks_x,
                cz + 1 < mesh.chunks_z,
                cx > 0,
            ];
            for m in &c.levels {
                let surface = (m.grid.0 * m.grid.1) as usize;
                for side in Side::ALL {
                    assert_eq!(m.skirt_bottom[side.index()].is_some(), has[side.index()]);
                }
                // The lower vertices all sit at one of the side bottoms.
                let bottoms: Vec<f32> = m.skirt_bottom.iter().flatten().copied().collect();
                for v in &m.vertices[surface..] {
                    assert!(bottoms.contains(&v.position[1]));
                }
                // Skirt triangles are vertical and face away from the chunk.
                let centre = Vec3::new(
                    (c.origin.0 + c.cells.0 / 2) as f32,
                    0.0,
                    (c.origin.1 + c.cells.1 / 2) as f32,
                );
                for t in m.skirt.chunks(3) {
                    let n = normal_of(m, t);
                    assert!(n.y.abs() <= 1e-4 * n.length().max(1.0), "{n}");
                    let mid = (Vec3::from(m.vertices[t[0] as usize].position)
                        + Vec3::from(m.vertices[t[1] as usize].position)
                        + Vec3::from(m.vertices[t[2] as usize].position))
                        / 3.0;
                    let outward = Vec3::new(mid.x - centre.x, 0.0, mid.z - centre.z);
                    assert!(
                        n.dot(outward) >= 0.0,
                        "skirt of chunk {:?} faces inward",
                        c.chunk
                    );
                }
            }
        }
    }
    // A map that is a single chunk has nobody to meet: no skirts at all.
    let single = build_mesh(&generate(32, 32, 1));
    assert!(single.chunks[0].levels.iter().all(|m| m.skirt.is_empty()));
}

// ---------------------------------------------------------------------------
// Packing
// ---------------------------------------------------------------------------

#[test]
fn packed_ranges_are_consecutive_per_level_and_in_chunk_order() {
    let map = generate(70, 70, 3);
    let mesh = build_mesh(&map);
    let packed = pack(&mesh);
    assert_eq!(packed.ranges.len(), LOD_LEVELS);
    let mut next = 0;
    for (level, per_chunk) in packed.ranges.iter().enumerate() {
        assert_eq!(per_chunk.len(), mesh.chunks.len());
        for (id, range) in per_chunk.iter().enumerate() {
            assert_eq!(range.start, next, "level {level} chunk {id}");
            next = range.end;
            let expect = mesh.chunks[id]
                .levels
                .get(level)
                .map_or(0, |m| m.surface.len() + m.skirt.len());
            assert_eq!(range.len(), expect);
        }
    }
    assert_eq!(next as usize, packed.indices.len());
    assert!(
        packed
            .indices
            .iter()
            .all(|&i| (i as usize) < packed.vertices.len())
    );
    // Indices of a range point at vertices of that chunk and level.
    for (level, per_chunk) in packed.ranges.iter().enumerate() {
        for (id, range) in per_chunk.iter().enumerate() {
            for &i in &packed.indices[range.start as usize..range.end as usize] {
                let v = &packed.vertices[i as usize];
                assert_eq!(v.lod as usize, level);
                let c = &mesh.chunks[id];
                let (x, z) = (v.position[0], v.position[2]);
                assert!(
                    x >= c.origin.0 as f32
                        && x <= (c.origin.0 + c.cells.0) as f32
                        && z >= c.origin.1 as f32
                        && z <= (c.origin.1 + c.cells.1) as f32
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Normals and textures
// ---------------------------------------------------------------------------

#[test]
fn normals_of_a_flat_and_a_sloped_map() {
    let flat = Map::from_parts(4, 4, 5.0, vec!["a".into()], vec![1000; 25], vec![0; 16]).unwrap();
    for vz in 0..=4 {
        for vx in 0..=4 {
            let n = vertex_normal(&flat, vx, vz);
            assert!((n[1] - 1.0).abs() < 1e-6, "{n:?}");
        }
    }
    // Height rising 1 m per metre to the east: normal tilts to the west.
    let heights: Vec<u16> = (0..25).map(|i| ((i % 5) * 13107) as u16).collect();
    let ramp = Map::from_parts(4, 4, 5.0, vec!["a".into()], heights, vec![0; 16]).unwrap();
    for vx in 0..=4 {
        let n = vertex_normal(&ramp, vx, 2);
        assert!(
            (n[0] + std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3,
            "{n:?}"
        );
        assert!(
            (n[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3,
            "{n:?}"
        );
        assert!(n[2].abs() < 1e-6);
    }
    // A ramp has the same slope over any step (away from the border).
    let n1 = vertex_normal_step(&ramp, 2, 2, 1);
    let n2 = vertex_normal_step(&ramp, 2, 2, 2);
    assert!((0..3).all(|k| (n1[k] - n2[k]).abs() < 1e-5));
}

#[test]
fn coarse_levels_shade_over_their_own_step() {
    // A single spike one vertex wide: full-detail normals next to it point
    // away from it, a level with step 4 sees it only as a gentle bump.
    let mut heights = vec![0_u16; 33 * 33];
    heights[16 * 33 + 16] = 40_000;
    let map = Map::from_parts(32, 32, 10.0, vec!["a".into()], heights, vec![0; 32 * 32]).unwrap();
    let steep = vertex_normal_step(&map, 17, 16, 1);
    let soft = vertex_normal_step(&map, 16, 16, 4);
    assert!(steep[1] < 0.99 && steep[0] > 0.3, "{steep:?}");
    assert!(soft[1] > 0.99, "{soft:?}");
    let mesh = build_mesh(&map);
    // Level 2 has a vertex at (16, 16): its stored normal is the step-4 one.
    let v = mesh.chunks[0].level(2).grid_vertex(4, 4);
    assert_eq!(v.normal, vertex_normal_step(&map, 16, 16, 4));
}

#[test]
fn textures_tile_and_differ() {
    let names = [
        "grass",
        "dirt",
        "rock",
        "sand",
        "dark_ground",
        "unknown_thing",
    ];
    let textures: Vec<Vec<u8>> = names.iter().map(|n| material_texture(n)).collect();
    for (i, t) in textures.iter().enumerate() {
        assert_eq!(t.len(), (TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
        let distinct: std::collections::BTreeSet<_> =
            t.chunks(4).map(|p| (p[0], p[1], p[2])).collect();
        assert!(distinct.len() > 50, "{} is flat", names[i]);
        for other in &textures[..i] {
            assert_ne!(t, other);
        }
    }
    // Tileable: the step across the wrap equals typical neighbour steps.
    let t = &textures[0];
    let n = TEXTURE_SIZE as usize;
    let px = |x: usize, y: usize| i32::from(t[(y * n + x) * 4 + 1]);
    let mean_step: i32 = (0..n).map(|y| (px(1, y) - px(0, y)).abs()).sum::<i32>() / n as i32;
    let wrap: i32 = (0..n).map(|y| (px(0, y) - px(n - 1, y)).abs()).sum::<i32>() / n as i32;
    assert!(wrap <= mean_step * 2 + 2, "wrap {wrap} vs step {mean_step}");
}

#[test]
fn mip_chain_reaches_one_texel() {
    let levels = mip_chain(material_texture("grass"), TEXTURE_SIZE);
    assert_eq!(levels.len(), 8);
    assert_eq!(levels.last().unwrap().len(), 4);
    assert_eq!(levels[3].len(), 16 * 16 * 4);
}
