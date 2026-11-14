//! Local tuning aid (not committed): triangles, team share and size per model; arg = name filter.
use sf_asset_check::Checker;
use sf_model_gen::{glb, models::MODELS};
fn main() {
    let f = std::env::args().nth(1).unwrap_or_default();
    let c = Checker::embedded();
    for b in MODELS {
        let m = b();
        if !m.stem.contains(&f) {
            continue;
        }
        let r = c.check_bytes(&format!("{}.glb", m.stem), &glb::write(&m));
        let (lo, hi) = r.facts.bounds.unwrap();
        let e = hi - lo;
        let own = |n: &str| m.lods[0].find(n).map_or(0, |x| x.mesh.len());
        println!("  own: hull {} turret {} barrel {}", own("hull"), own("turret"), own("barrel"));
        println!(
            "{:34} tris {:?} team {:.3?} size {:.2}x{:.2}x{:.2} lo.y {:.2} findings {:?}",
            m.stem, m.triangles(), r.facts.team_share, e.x, e.y, e.z, lo.y,
            r.findings.iter().map(|x| x.message.clone()).collect::<Vec<_>>()
        );
    }
}
