//! The harvest rules file (WP7.2): the shipped numbers are pinned, and bad numbers are refused.

use sf_data::harvest::{HarvestError, HarvestSettings};

/// The numbers of `assets/config/harvest.ron`, written out. Changing the file means changing
/// this test on purpose (and the checksum of any scenario that uses the shipped file).
#[test]
fn the_shipped_harvest_numbers_are_pinned() {
    let s = HarvestSettings::embedded();
    assert_eq!(s.capacity, 200);
    assert_eq!(s.mine_amount, 4);
    assert_eq!(s.mine_period, 8);
    assert_eq!(s.unload_amount, 10);
    assert_eq!(s.unload_period, 4);
    assert_eq!(s.search_radius, 40);
    assert_eq!(s.retry_period, 100);
    assert_eq!(s.call_range, 6);
    assert_eq!(s.grant_timeout, 400);
}

/// The timings the comments of the file state, worked out from the numbers.
#[test]
fn the_shipped_timings_are_the_ones_the_file_describes() {
    let s = HarvestSettings::embedded();
    let ticks_per_second = 20u32;
    // 10 ore per second while mining; 400 ticks, 20 seconds, to fill.
    let mine_per_second = u32::from(s.mine_amount) * ticks_per_second / u32::from(s.mine_period);
    assert_eq!(mine_per_second, 10);
    let fill_ticks =
        u32::from(s.capacity).div_ceil(u32::from(s.mine_amount)) * u32::from(s.mine_period);
    assert_eq!(fill_ticks, 400);
    // 50 ore per second while unloading; 80 ticks, 4 seconds, to empty a full load.
    let unload_per_second =
        u32::from(s.unload_amount) * ticks_per_second / u32::from(s.unload_period);
    assert_eq!(unload_per_second, 50);
    let unload_ticks =
        u32::from(s.capacity).div_ceil(u32::from(s.unload_amount)) * u32::from(s.unload_period);
    assert_eq!(unload_ticks, 80);
}

/// A grant must outlast the drive from the call range, or no harvester would ever arrive. The
/// harvester's speed is in `unit_defs.ron` (1.2 cells per second).
#[test]
fn the_grant_lasts_longer_than_the_drive_from_the_call_range() {
    let s = HarvestSettings::embedded();
    let defs = sf_data::units::UnitDefs::embedded();
    let harvester = defs.sim_unit(defs.kind_of("harvester").unwrap()).unwrap();
    // The shortest the drive can be is the call range at full speed in a straight line.
    let ticks = (sf_math::Fx::from_int(i32::from(s.call_range)).raw()
        / harvester.speed_per_tick.raw()) as u32;
    assert!(ticks > 0);
    assert!(
        u32::from(s.grant_timeout) > 2 * ticks,
        "{ticks} ticks to drive"
    );
}

#[test]
fn bad_numbers_are_refused() {
    let with = |c: u16, ma: u16, mp: u16, ua: u16, up: u16, r: u16, rp: u16, cr: u16, gt: u16| {
        HarvestSettings::from_ron(&format!(
            "(capacity: {c}, mine_amount: {ma}, mine_period: {mp}, unload_amount: {ua}, \
             unload_period: {up}, search_radius: {r}, retry_period: {rp}, call_range: {cr}, \
             grant_timeout: {gt})"
        ))
    };
    assert!(with(200, 4, 8, 10, 4, 40, 100, 6, 400).is_ok());
    // Every number is at least 1.
    assert!(matches!(
        with(0, 4, 8, 10, 4, 40, 100, 6, 400),
        Err(HarvestError::Invalid(_))
    ));
    assert!(matches!(
        with(200, 0, 8, 10, 4, 40, 100, 6, 400),
        Err(HarvestError::Invalid(_))
    ));
    assert!(matches!(
        with(200, 4, 0, 10, 4, 40, 100, 6, 400),
        Err(HarvestError::Invalid(_))
    ));
    assert!(matches!(
        with(200, 4, 8, 0, 4, 40, 100, 6, 400),
        Err(HarvestError::Invalid(_))
    ));
    assert!(matches!(
        with(200, 4, 8, 10, 0, 40, 100, 6, 400),
        Err(HarvestError::Invalid(_))
    ));
    assert!(matches!(
        with(200, 4, 8, 10, 4, 0, 100, 6, 400),
        Err(HarvestError::Invalid(_))
    ));
    assert!(matches!(
        with(200, 4, 8, 10, 4, 40, 0, 6, 400),
        Err(HarvestError::Invalid(_))
    ));
    assert!(matches!(
        with(200, 4, 8, 10, 4, 40, 100, 0, 400),
        Err(HarvestError::Invalid(_))
    ));
    assert!(matches!(
        with(200, 4, 8, 10, 4, 40, 100, 6, 0),
        Err(HarvestError::Invalid(_))
    ));
    // A bite is no bigger than the load.
    assert!(with(200, 200, 8, 10, 4, 40, 100, 6, 400).is_ok());
    assert!(matches!(
        with(200, 201, 8, 10, 4, 40, 100, 6, 400),
        Err(HarvestError::Invalid(_))
    ));
    // A missing field and an unknown one.
    assert!(matches!(
        HarvestSettings::from_ron("(capacity: 1)"),
        Err(HarvestError::Ron(_))
    ));
    assert!(matches!(
        HarvestSettings::from_ron(
            "(capacity: 200, mine_amount: 4, mine_period: 8, unload_amount: 10, unload_period: 4, \
             search_radius: 40, retry_period: 100, call_range: 6, grant_timeout: 400, x: 1)"
        ),
        Err(HarvestError::Ron(_))
    ));
}
