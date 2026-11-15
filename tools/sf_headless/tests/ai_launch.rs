//! WP9.3b-0: the frozen opponent of the launch-timing gate (`docs/rules/ai.md`, "Launch timing (WP9.3b-0)").
//!
//! `tests/fixtures/normal_9_3c.ron` is a byte copy of the shipped `assets/ai/normal.ron` at main 7031a82 (after WP9.3c). It
//! is the opponent of every duel row of the gate (a profile per seat, as `ai_duel.rs` does it) and a test fixture, no
//! shipped profile. These tests pin it: its goal lists and the launch numbers as literals, and its equality with the
//! shipped profile except for the numbers the work package deliberately moved (none while the shipped file is unchanged).

use std::path::{Path, PathBuf};

use sf_data::ai_profile::AiProfile;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/normal_9_3c.ron")
}

fn frozen() -> AiProfile {
    AiProfile::from_ron(&std::fs::read_to_string(fixture()).unwrap()).unwrap()
}

#[test]
fn the_frozen_opponent_has_the_goal_lists_and_launch_numbers_of_wp9_3c() {
    let f = frozen();
    let list = [
        ("power_plant", 1),
        ("refinery", 1),
        ("vehicle_factory", 1),
        ("barracks", 1),
        ("power_plant", 2),
        ("refinery", 2),
    ];
    for faction in &f.factions {
        let got: Vec<(&str, u16)> = faction
            .build_order
            .iter()
            .map(|(k, n)| (k.as_str(), *n))
            .collect();
        assert_eq!(got, list);
    }
    assert_eq!(f.factions.len(), 2);
    let a = &f.army;
    assert_eq!(
        (
            a.attack_value,
            a.max_wait,
            a.min_units,
            a.first_attack_tick,
            a.army_floor_value,
            a.reinforce_value
        ),
        (2000, 12000, 4, 2400, 2000, 1000)
    );
    assert_eq!(
        (a.stage_distance, a.stage_permille, a.stage_patience),
        (14, 700, 1800)
    );
    assert_eq!(
        (a.escalate_permille, a.attack_value_max, a.harvester_threat),
        (0, 0, false)
    );
}

#[test]
fn the_frozen_opponent_is_the_shipped_profile_but_for_the_numbers_this_wp_moved() {
    // Byte copy: the fixture parses to exactly the shipped profile (its name, "normal", is the shipped one). If a later work
    // package changes the shipped file, it must copy the old numbers into `shipped` here, as `ai_duel.rs` does, and say why.
    let shipped = AiProfile::embedded();
    assert_eq!(frozen(), shipped);
}
