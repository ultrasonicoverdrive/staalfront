mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_sim::PlayerId;

#[test]
fn measure() {
    for gate in [false, true] {
        let mut s = Scenario::load(&scenario_path("match.ron")).unwrap();
        s.vision.as_mut().unwrap().acquire_needs_sight = gate;
        s.ticks = 12000;
        let mut w = s.build_world().unwrap();
        let mut lost = 0;
        let mut first_death = None;
        let mut result = None;
        for _ in 0..s.ticks {
            let r = w.step();
            if first_death.is_none() && r.deaths > 0 {
                first_death = Some(r.tick.0);
            }
            lost += w.deaths().iter().filter(|d| d.owner == PlayerId(0)).count();
            if let Some(res) = w.match_result() {
                result = Some((w.tick().0 - 1, res.verdict, lost));
                break;
            }
        }
        println!("MEASURE gate={gate} first_death={first_death:?} result={result:?}");
    }
}
