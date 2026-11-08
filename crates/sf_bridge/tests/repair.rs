//! The repair and sale rules through the bridge (WP6.6): the data numbers arrive in the world
//! unchanged, each one as itself, the limits of the data and of the simulation are the same, and
//! the shipped file is what the world takes.

use sf_bridge::repair_rules;
use sf_data::repair::{self, RepairSettings};
use sf_sim::RepairRules;

#[test]
fn each_repair_number_arrives_as_itself() {
    // Distinct values, so a swapped or dropped field shows.
    let settings = RepairSettings {
        hp_per_tick: 7,
        full_cost_permille: 321,
        sell_ticks: 43,
        refund_permille: 654,
    };
    assert_eq!(
        repair_rules(&settings),
        RepairRules {
            hp_per_tick: 7,
            full_cost_permille: 321,
            sell_ticks: 43,
            refund_permille: 654,
        }
    );
}

#[test]
fn the_limits_of_the_data_are_those_of_the_simulation() {
    assert_eq!(repair::MAX_HP_PER_TICK, sf_sim::MAX_REPAIR_HP_PER_TICK);
    assert_eq!(repair::MAX_SELL_TICKS, sf_sim::MAX_SELL_TICKS);
}

#[test]
fn the_shipped_numbers_are_valid_for_the_simulation() {
    assert!(repair_rules(&RepairSettings::embedded()).is_valid());
}
