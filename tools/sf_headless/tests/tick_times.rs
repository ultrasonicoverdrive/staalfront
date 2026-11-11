//! Measurement for WP1.16a (suspect 4, the sim thread in M1): the time of every tick of the pinned
//! `match` scenario, and which ticks are the slowest. Not run in CI (timings); run it with
//! `cargo test --release -p sf_headless --test tick_times -- --ignored --nocapture`. The clock is
//! read here, in the test, never in `sf_sim`.

mod common;

use std::time::Instant;

use common::scenario_path;
use sf_headless::scenario::Scenario;

#[test]
#[ignore = "measurement, prints numbers"]
fn the_slowest_ticks_of_the_match() {
    let s = Scenario::load(&scenario_path("match.ron")).unwrap();
    let mut world = s.build_world().unwrap();
    let mut times = Vec::new();
    for _ in 0..s.ticks {
        let started = Instant::now();
        world.step();
        times.push((started.elapsed().as_secs_f64() * 1e3, world.tick().0 - 1));
    }
    let mut ms: Vec<f64> = times.iter().map(|t| t.0).collect();
    ms.sort_by(f64::total_cmp);
    let pct = |p: f64| ms[((ms.len() as f64 * p) as usize).min(ms.len() - 1)];
    println!(
        "{} ticks: avg {:.3} ms, p50 {:.3}, p99 {:.3}, p99.9 {:.3}, max {:.3}",
        ms.len(),
        ms.iter().sum::<f64>() / ms.len() as f64,
        pct(0.5),
        pct(0.99),
        pct(0.999),
        ms[ms.len() - 1]
    );
    times.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (t, tick) in times.iter().take(12) {
        println!("  tick {tick}: {t:.3} ms");
    }
}
