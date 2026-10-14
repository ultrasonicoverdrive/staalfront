//! Known `Display`/`Debug` output of `Fx` (decimal literals live here, in
//! `tests/`, because the determinism lint keeps them out of `src/`).

use sf_math::{Fx, Vec2};

#[test]
fn display_is_the_exact_decimal_expansion() {
    assert_eq!(format!("{}", Fx::ZERO), "0");
    assert_eq!(format!("{}", Fx::from_int(3)), "3");
    assert_eq!(format!("{}", Fx::from_int(-3)), "-3");
    assert_eq!(format!("{}", Fx::HALF), "0.5");
    assert_eq!(format!("{}", Fx::from_ratio(-3, 2)), "-1.5");
    assert_eq!(format!("{}", Fx::from_ratio(1, 4)), "0.25");
    assert_eq!(format!("{}", Fx::EPSILON), "0.0000152587890625");
    assert_eq!(format!("{}", Fx::from_raw(-1)), "-0.0000152587890625");
    assert_eq!(format!("{}", Fx::LEPTON), "0.00390625");
    assert_eq!(format!("{}", Fx::MIN), "-140737488355328");
    assert_eq!(format!("{}", Fx::MAX), "140737488355327.9999847412109375");
}

#[test]
fn debug_wraps_the_display() {
    assert_eq!(format!("{:?}", Fx::from_ratio(1, 4)), "Fx(0.25)");
    assert_eq!(
        format!("{:?}", Vec2::new(Fx::from_ratio(3, 2), Fx::from_int(-2))),
        "Vec2(1.5, -2)"
    );
}
