mod common;
use common::scenario_path;
use sf_headless::run::{RunOptions, run_watched};
use sf_headless::scenario::Scenario;
use sf_sim::PlayerId;

#[test]
fn explore() {
    let name = std::env::var("SCN").unwrap_or("ai_base.ron".into());
    let s = Scenario::load(&scenario_path(&name)).unwrap();
    let ticks: u64 = std::env::var("TICKS")
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(s.ticks);
    let mut seen = std::collections::BTreeMap::new();
    let f = run_watched(&s, &RunOptions { ticks, every: 100 }, |w| {
        for v in w.snapshot().iter() {
            if v.owner != PlayerId(0) {
                continue;
            }
            let st = w.stands(v.id);
            let e = seen.entry(v.id).or_insert((false, false));
            if !e.0 {
                e.0 = true;
                println!(
                    "t{} new {:?} kind {} cell {:?} credits {} power {:?}",
                    w.tick().0,
                    v.id,
                    v.kind.0,
                    w.cell_of(v.id),
                    w.credits(PlayerId(0)),
                    w.power_of(PlayerId(0))
                );
            }
            if st && !e.1 && w.footprint_of(v.id).is_some() {
                e.1 = true;
                println!("t{} stands kind {}", w.tick().0, v.kind.0);
            }
        }
        if !w.stalled().is_empty() && w.tick().0 % 100 == 0 {
            println!("t{} stalled {:?}", w.tick().0, w.stalled());
        }
    })
    .unwrap();
    for c in &f.ai_commands {
        println!("cmd {:?}", c);
    }
    println!(
        "ignored {} executed {} previews {} digest {:x} credits {} delivered {}",
        f.outcome.commands_ignored,
        f.outcome.commands_executed,
        f.ais[0].state().previews,
        f.ais[0].digest(),
        f.world.credits(PlayerId(0)),
        f.world.ore_delivered(PlayerId(0))
    );
    println!(
        "blocked {:?} refused {:?}",
        f.ais[0].state().blocked_until,
        f.ais[0].state().refused
    );
}
