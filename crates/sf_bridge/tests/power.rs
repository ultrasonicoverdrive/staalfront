//! The power rules through the bridge (WP6.4): the data numbers arrive in the world unchanged, each one as
//! itself, and the shipped file is what the world takes.

use sf_bridge::power_rules;
use sf_data::power::PowerSettings;
use sf_sim::PowerRules;

#[test]
fn each_power_number_arrives_as_itself() {
    // Distinct values, so a swapped or dropped field shows.
    let settings = PowerSettings {
        shortage_floor_permille: 321,
        defence_cutoff_permille: 654,
    };
    assert_eq!(
        power_rules(&settings),
        PowerRules {
            shortage_floor_permille: 321,
            defence_cutoff_permille: 654
        }
    );
}

#[test]
fn the_shipped_numbers_are_valid_for_the_simulation() {
    assert!(power_rules(&PowerSettings::embedded()).is_valid());
}
