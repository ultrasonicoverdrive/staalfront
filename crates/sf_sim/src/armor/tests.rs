use super::*;

const BULLET: u16 = 0;
const SHELL: u16 = 4;
const SOFT: u16 = 1;
const HARD: u16 = 3;

/// Two damage types against two armor types, a complete grid.
fn cells() -> Vec<(u16, u16, u16)> {
    vec![
        (BULLET, SOFT, 100),
        (BULLET, HARD, 10),
        (SHELL, SOFT, 80),
        (SHELL, HARD, 120),
    ]
}

fn table() -> ArmorTable {
    ArmorTable::new(
        vec![BULLET, SHELL],
        vec![SOFT, HARD],
        cells(),
        vec![(Kind(7), HARD), (Kind(2), SOFT)],
    )
    .unwrap()
}

fn refused(
    damage_types: Vec<u16>,
    armor_types: Vec<u16>,
    cells: Vec<(u16, u16, u16)>,
    units: Vec<(Kind, u16)>,
) -> ArmorError {
    ArmorTable::new(damage_types, armor_types, cells, units).unwrap_err()
}

#[test]
fn a_complete_table_answers_every_pair_and_every_unit() {
    let t = table();
    assert_eq!(t.percent(BULLET, SOFT), Some(100));
    assert_eq!(t.percent(BULLET, HARD), Some(10));
    assert_eq!(t.percent(SHELL, SOFT), Some(80));
    assert_eq!(t.percent(SHELL, HARD), Some(120));
    assert_eq!(t.percent(9, SOFT), None);
    assert_eq!(t.percent(BULLET, 9), None);
    assert_eq!(t.armor_of(Kind(7)), Some(HARD));
    assert_eq!(t.armor_of(Kind(2)), Some(SOFT));
    assert_eq!(t.armor_of(Kind(3)), None);
    assert_eq!(t.damage_types(), &[BULLET, SHELL]);
    assert_eq!(t.armor_types(), &[SOFT, HARD]);
    assert_eq!(t.unit_kinds().collect::<Vec<_>>(), vec![Kind(2), Kind(7)]);
}

#[test]
fn the_order_of_the_input_does_not_matter() {
    let a = table();
    let mut shuffled = cells();
    shuffled.reverse();
    let b = ArmorTable::new(
        vec![SHELL, BULLET],
        vec![HARD, SOFT],
        shuffled,
        vec![(Kind(2), SOFT), (Kind(7), HARD)],
    )
    .unwrap();
    assert_eq!(a, b);
    assert_eq!(a.digest(), b.digest());
}

#[test]
fn any_change_to_the_table_changes_its_digest() {
    let base = table().digest();
    let mut changed = cells();
    changed[1].2 = 11;
    let t = ArmorTable::new(
        vec![BULLET, SHELL],
        vec![SOFT, HARD],
        changed,
        vec![(Kind(7), HARD), (Kind(2), SOFT)],
    )
    .unwrap();
    assert_ne!(t.digest(), base, "a percentage");
    let t = ArmorTable::new(
        vec![BULLET, SHELL],
        vec![SOFT, HARD],
        cells(),
        vec![(Kind(7), SOFT), (Kind(2), SOFT)],
    )
    .unwrap();
    assert_ne!(t.digest(), base, "the armor type of a unit");
    let t = ArmorTable::new(
        vec![BULLET, SHELL],
        vec![SOFT, HARD],
        cells(),
        vec![(Kind(7), HARD)],
    )
    .unwrap();
    assert_ne!(t.digest(), base, "a unit more or less");
}

#[test]
fn a_table_without_types_is_refused() {
    assert_eq!(
        refused(vec![], vec![SOFT], vec![], vec![]),
        ArmorError::Empty
    );
    assert_eq!(
        refused(vec![BULLET], vec![], vec![], vec![]),
        ArmorError::Empty
    );
}

#[test]
fn a_type_listed_twice_is_refused() {
    assert_eq!(
        refused(vec![BULLET, BULLET], vec![SOFT], vec![], vec![]),
        ArmorError::DuplicateDamageType(BULLET)
    );
    assert_eq!(
        refused(vec![BULLET], vec![SOFT, SOFT], vec![], vec![]),
        ArmorError::DuplicateArmorType(SOFT)
    );
}

#[test]
fn a_percentage_for_an_unknown_id_is_refused() {
    let mut c = cells();
    c.push((77, SOFT, 100));
    assert_eq!(
        refused(vec![BULLET, SHELL], vec![SOFT, HARD], c, vec![]),
        ArmorError::UnknownDamageType(77)
    );
    let mut c = cells();
    c.push((BULLET, 88, 100));
    assert_eq!(
        refused(vec![BULLET, SHELL], vec![SOFT, HARD], c, vec![]),
        ArmorError::UnknownArmorType(88)
    );
}

#[test]
fn a_unit_with_an_unknown_armor_type_is_refused() {
    assert_eq!(
        refused(
            vec![BULLET, SHELL],
            vec![SOFT, HARD],
            cells(),
            vec![(Kind(1), 99)]
        ),
        ArmorError::UnknownArmorType(99)
    );
}

#[test]
fn a_unit_with_two_armor_types_is_refused() {
    assert_eq!(
        refused(
            vec![BULLET, SHELL],
            vec![SOFT, HARD],
            cells(),
            vec![(Kind(1), SOFT), (Kind(1), HARD)]
        ),
        ArmorError::DuplicateUnit(Kind(1))
    );
}

#[test]
fn a_pair_with_two_percentages_is_refused() {
    let mut c = cells();
    c.push((SHELL, HARD, 50));
    assert_eq!(
        refused(vec![BULLET, SHELL], vec![SOFT, HARD], c, vec![]),
        ArmorError::DuplicatePercent {
            damage_type: SHELL,
            armor_type: HARD
        }
    );
}

#[test]
fn a_hole_in_the_table_is_refused_and_named() {
    let mut c = cells();
    c.retain(|&(d, a, _)| !(d == SHELL && a == SOFT));
    assert_eq!(
        refused(vec![BULLET, SHELL], vec![SOFT, HARD], c, vec![]),
        ArmorError::MissingPercent {
            damage_type: SHELL,
            armor_type: SOFT
        }
    );
    // A whole damage type with no row.
    assert_eq!(
        refused(
            vec![BULLET, SHELL],
            vec![SOFT, HARD],
            cells()[..2].to_vec(),
            vec![]
        ),
        ArmorError::MissingPercent {
            damage_type: SHELL,
            armor_type: SOFT
        }
    );
}

#[test]
fn a_percentage_above_the_bound_is_refused_and_the_bound_itself_is_fine() {
    let mut c = cells();
    c[0].2 = MAX_PERCENT + 1;
    assert_eq!(
        refused(vec![BULLET, SHELL], vec![SOFT, HARD], c, vec![]),
        ArmorError::PercentTooHigh {
            damage_type: BULLET,
            armor_type: SOFT,
            percent: MAX_PERCENT + 1
        }
    );
    let mut c = cells();
    c[0].2 = MAX_PERCENT;
    c[1].2 = 0;
    let t = ArmorTable::new(vec![BULLET, SHELL], vec![SOFT, HARD], c, vec![]).unwrap();
    assert_eq!(t.percent(BULLET, SOFT), Some(MAX_PERCENT));
    assert_eq!(t.percent(BULLET, HARD), Some(0));
}

#[test]
fn every_error_has_a_message() {
    let errors = [
        ArmorError::Empty,
        ArmorError::DuplicateDamageType(1),
        ArmorError::DuplicateArmorType(2),
        ArmorError::UnknownDamageType(3),
        ArmorError::UnknownArmorType(4),
        ArmorError::DuplicatePercent {
            damage_type: 5,
            armor_type: 6,
        },
        ArmorError::MissingPercent {
            damage_type: 7,
            armor_type: 8,
        },
        ArmorError::PercentTooHigh {
            damage_type: 9,
            armor_type: 10,
            percent: 401,
        },
        ArmorError::DuplicateUnit(Kind(11)),
    ];
    for e in errors {
        assert!(!e.to_string().is_empty(), "{e:?}");
    }
}

// ---- the arithmetic ------------------------------------------------------------------------

#[test]
fn a_hundred_percent_and_no_falloff_is_the_damage_itself() {
    for damage in [1, 2, 6, 45, 90, 5000, 1_000_000] {
        assert_eq!(scaled_damage(damage, 100, 1, 1), damage);
    }
}

#[test]
fn rounding_is_to_the_nearest_with_a_tie_going_up() {
    // 7 * 50 / 100 = 7/2 -> 4; 5 * 50 / 100 = 5/2 -> 3; 9 * 50 / 100 = 9/2 -> 5 (ties go up)
    assert_eq!(scaled_damage(7, 50, 1, 1), 4);
    assert_eq!(scaled_damage(5, 50, 1, 1), 3);
    assert_eq!(scaled_damage(9, 50, 1, 1), 5);
    // 6 * 35 / 100 = 21/10 -> 2; 6 * 45 / 100 = 27/10 -> 3; 6 * 25 / 100 = 3/2 -> 2 (a tie up)
    assert_eq!(scaled_damage(6, 35, 1, 1), 2);
    assert_eq!(scaled_damage(6, 45, 1, 1), 3);
    assert_eq!(scaled_damage(6, 25, 1, 1), 2);
}

#[test]
fn a_fraction_of_a_falloff_is_part_of_the_same_rounding() {
    // 90 * 100% * 1/3 = 30; 80 * 120% * 1/3 = 32; 7 * 100% * 1/2 = 7/2 -> 4 (a tie up)
    assert_eq!(scaled_damage(90, 100, 1, 3), 30);
    assert_eq!(scaled_damage(80, 120, 1, 3), 32);
    assert_eq!(scaled_damage(7, 100, 1, 2), 4);
    // One rounding, not two: 5 * 90% is 9/2 and rounding that first would give 5 and then 3 for the
    // half; the whole product is 9/4, which rounds down to two
    assert_eq!(scaled_damage(5, 90, 1, 2), 2);
}

#[test]
fn a_hit_is_never_rounded_to_nothing_unless_a_factor_is_zero() {
    assert_eq!(scaled_damage(4, 10, 1, 1), 1, "4/10 would be 0");
    assert_eq!(scaled_damage(1, 1, 1, 1), 1, "a hundredth of one");
    assert_eq!(
        scaled_damage(1, 100, 1, 1_000_000),
        1,
        "a sliver of a falloff"
    );
    assert_eq!(scaled_damage(4, 0, 1, 1), 0, "immune");
    assert_eq!(scaled_damage(4, 100, 0, 7), 0, "no falloff left");
    assert_eq!(scaled_damage(0, 100, 1, 1), 0, "no damage");
}

#[test]
fn a_huge_hit_saturates_instead_of_wrapping() {
    assert_eq!(scaled_damage(u32::MAX, MAX_PERCENT, 1, 1), u32::MAX);
    assert_eq!(
        scaled_damage(1_000_000, MAX_PERCENT, 1, 1),
        4_000_000,
        "the bound of the data is four million"
    );
}
