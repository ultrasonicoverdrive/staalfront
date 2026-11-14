mod common;
use common::scenario_path;
use sf_headless::run::{RunOptions, run_observed};
use sf_headless::scenario::Scenario;

#[test]
fn probe() {
    let s = Scenario::load(&scenario_path("ai_bounce.ron")).unwrap();
    let mut last = (0, 0, 0);
    run_observed(
        &s,
        &RunOptions {
            ticks: s.ticks,
            every: 100,
        },
        |w, ais| {
            let st = ais[0].state();
            let cur = (st.attacks_launched, st.bounces, st.groups.len() as u32);
            if cur != last {
                last = cur;
                let lv: Vec<u64> = st.groups.iter().map(|g| g.launch_value).collect();
                println!(
                    "t={} launched={} bounces={} bv={} bt={} groups={:?} members={:?}",
                    w.tick().0,
                    st.attacks_launched,
                    st.bounces,
                    st.bounce_value,
                    st.bounce_tick.0,
                    lv,
                    st.groups
                        .iter()
                        .map(|g| (g.members.len(), g.stage as u8))
                        .collect::<Vec<_>>()
                );
            }
        },
    )
    .unwrap();
}
