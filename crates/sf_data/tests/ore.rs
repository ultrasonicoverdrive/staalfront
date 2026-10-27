//! The ore rules file (WP7.1): the shipped numbers are pinned, and bad numbers are refused.

use sf_data::ore::{MAX_REGROW_PERIOD, OreError, OreSettings};

/// The numbers of `assets/config/ore.ron`, written out. Changing the file means changing
/// this test on purpose (and the checksum of any scenario that uses the shipped file).
#[test]
fn the_shipped_ore_numbers_are_pinned() {
    let s = OreSettings::embedded();
    assert_eq!(s.start, 120);
    assert_eq!(s.max, 160);
    assert_eq!(s.regrow_amount, 1);
    assert_eq!(s.regrow_period, 200);
}

#[test]
fn bad_numbers_are_refused() {
    let with = |start, max, amount, period| {
        OreSettings::from_ron(&format!(
            "(start: {start}, max: {max}, regrow_amount: {amount}, regrow_period: {period})"
        ))
    };
    assert!(with(10, 20, 1, 5).is_ok());
    assert!(
        with(0, 20, 0, 5).is_ok(),
        "an empty start and no growth are allowed"
    );
    assert!(matches!(with(0, 0, 1, 5), Err(OreError::Invalid(_))));
    assert!(matches!(with(21, 20, 1, 5), Err(OreError::Invalid(_))));
    assert!(matches!(with(10, 20, 1, 0), Err(OreError::Invalid(_))));
    assert!(with(10, 20, 1, MAX_REGROW_PERIOD).is_ok());
    assert!(matches!(
        with(10, 20, 1, MAX_REGROW_PERIOD + 1),
        Err(OreError::Invalid(_))
    ));
    assert!(matches!(
        OreSettings::from_ron("(start: 1, max: 2)"),
        Err(OreError::Ron(_))
    ));
    assert!(matches!(
        OreSettings::from_ron("(start: 1, max: 2, regrow_amount: 1, regrow_period: 1, x: 3)"),
        Err(OreError::Ron(_))
    ));
}
