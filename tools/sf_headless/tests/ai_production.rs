mod common;

use common::scenario_path;
use sf_headless::run::{RunOptions, run_observed};
use sf_headless::scenario::Scenario;
use sf_sim::{Command, PlayerId};

#[test]
fn explore() {
    let s = Scenario::load(&scenario_path("ai_scout.ron")).unwrap();
    let mut seen_anchor = None;
    let f = run_observed(
        &s,
        &RunOptions {
            ticks: 12000,
            every: 100,
        },
        |w, ais| {
            let t = w.tick().0;
            if seen_anchor.is_none()
                && ais[0]
                    .state()
                    .known_structures
                    .values()
                    .any(|k| k.owner == PlayerId(1) && k.kind.0 == 0)
            {
                seen_anchor = Some(t);
            }
            if t % 2000 == 0 {
                let mut counts = std::collections::BTreeMap::new();
                for v in w.snapshot().iter() {
                    if v.owner == PlayerId(0) && w.footprint_of(v.id).is_none() {
                        let c = w.cell_at(v.pos).unwrap();
                        *counts.entry(v.kind.0).or_insert(0) += 1;
                        if v.kind.0 == 15 {
                            eprintln!("  order {:?}", w.order_of(v.id));
                        }
                        if v.kind.0 == 15 || t == 12000 {
                            eprintln!("  t={t} kind {} at ({},{})", v.kind.0, c.x, c.y);
                        }
                    }
                }
                eprintln!(
                    "t={t} credits={} units={counts:?} gather={:?} scout={:?} leg={} known={}",
                    w.credits(PlayerId(0)),
                    ais[0].state().gather,
                    ais[0].state().scout,
                    ais[0].state().scout_leg,
                    ais[0].state().known_structures.len()
                );
            }
        },
    )
    .unwrap();
    eprintln!("anchor seen at {seen_anchor:?}");
    for (t, _, c) in &f.ai_commands {
        match c {
            Command::BuildStructure { .. } => {}
            _ => eprintln!("{t}: {c:?}"),
        }
    }
    eprintln!(
        "final {:x} digest {:x}",
        f.outcome.final_checksum, f.outcome.ai_digests[0].1
    );
}
