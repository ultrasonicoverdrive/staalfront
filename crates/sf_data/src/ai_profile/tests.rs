use super::*;

fn roles(list: &[(&str, u16)]) -> Vec<(String, u16)> {
    list.iter().map(|&(r, n)| (r.to_string(), n)).collect()
}

/// The shipped file with one text replaced (the replacement must be there).
fn edited(from: &str, to: &str) -> String {
    assert!(
        EMBEDDED_NORMAL.contains(from),
        "{from:?} is not in the file"
    );
    EMBEDDED_NORMAL.replacen(from, to, 1)
}

fn refused(text: &str, what: &str) {
    let e = AiProfile::from_ron(text).unwrap_err().to_string();
    assert!(e.contains(what), "{e:?} should mention {what:?}");
}

#[test]
fn the_shipped_profile_is_pinned() {
    // The literal values of assets/ai/normal.ron: a change to the file fails here, on purpose.
    let p = AiProfile::embedded();
    assert_eq!(p.name, "normal");
    assert_eq!((p.think_interval, p.stagger), (10, 3));
    let e = p.economy;
    assert_eq!(
        (
            e.power_margin,
            e.harvesters_per_refinery,
            e.reserve,
            e.max_parallel_builds,
            e.silo_percent,
            e.repair_below_percent,
            e.blocked_ticks,
            e.placement_cap
        ),
        (20, 2, 300, 1, 90, 70, 600, 256)
    );
    let a = p.army;
    assert_eq!(
        (
            a.attack_value,
            a.max_wait,
            a.min_units,
            a.first_attack_tick,
            a.defend_ticks,
            a.gather_distance
        ),
        (3000, 3600, 4, 2400, 200, 6)
    );
    assert_eq!(p.factions.len(), 2);
    let mix = roles(&[("rifleman", 4), ("rocket_trooper", 2), ("light_tank", 3)]);
    let meridian = &p.factions[0];
    assert_eq!(meridian.faction, Faction::Meridian);
    assert_eq!(
        meridian.build_order,
        roles(&[
            ("power_plant", 1),
            ("refinery", 1),
            ("barracks", 1),
            ("power_plant", 2),
            ("vehicle_factory", 1),
            ("defence_tower", 1),
            ("refinery", 2),
            ("radar", 1),
            ("defence_tower", 2),
            ("advanced_defences", 1),
        ])
    );
    assert_eq!(meridian.unit_mix, mix);
    let veyran = &p.factions[1];
    assert_eq!(veyran.faction, Faction::Veyran);
    assert_eq!(
        veyran.build_order,
        roles(&[
            ("power_plant", 1),
            ("refinery", 1),
            ("barracks", 1),
            ("power_plant", 2),
            ("vehicle_factory", 1),
            ("defence_tower", 1),
            ("fortified_bunker", 1),
            ("refinery", 2),
            ("radar", 1),
            ("defence_tower", 2),
            ("advanced_defences", 1),
        ])
    );
    assert_eq!(veyran.unit_mix, mix);
}

#[test]
fn the_shipped_profile_fits_the_shipped_definitions() {
    AiProfile::embedded().check(&UnitDefs::embedded()).unwrap();
}

#[test]
fn every_number_is_checked_when_the_file_is_read() {
    refused(
        &edited("think_interval: 10", "think_interval: 0"),
        "think_interval",
    );
    refused(
        &edited("think_interval: 10", "think_interval: 601"),
        "think_interval",
    );
    // A stagger that lets two of eight players think together: 10 / gcd(10, 5) = 2.
    refused(&edited("stagger: 3", "stagger: 5"), "stagger");
    refused(&edited("stagger: 3", "stagger: 0"), "stagger");
    refused(
        &edited("power_margin: 20", "power_margin: 1001"),
        "power_margin",
    );
    refused(
        &edited("harvesters_per_refinery: 2", "harvesters_per_refinery: 0"),
        "harvesters_per_refinery",
    );
    refused(
        &edited("max_parallel_builds: 1", "max_parallel_builds: 5"),
        "max_parallel_builds",
    );
    refused(
        &edited("silo_percent: 90", "silo_percent: 101"),
        "silo_percent",
    );
    refused(
        &edited("repair_below_percent: 70", "repair_below_percent: 0"),
        "repair_below_percent",
    );
    refused(
        &edited("blocked_ticks: 600", "blocked_ticks: 0"),
        "blocked_ticks",
    );
    refused(
        &edited("placement_cap: 256", "placement_cap: 0"),
        "placement_cap",
    );
    refused(
        &edited("attack_value: 3000", "attack_value: 0"),
        "attack_value",
    );
    refused(&edited("max_wait: 3600", "max_wait: 0"), "max_wait");
    refused(&edited("min_units: 4", "min_units: 0"), "min_units");
    refused(
        &edited("first_attack_tick: 2400", "first_attack_tick: 72001"),
        "first_attack_tick",
    );
    refused(
        &edited("defend_ticks: 200", "defend_ticks: 0"),
        "defend_ticks",
    );
    refused(
        &edited("gather_distance: 6", "gather_distance: 65"),
        "gather_distance",
    );
    refused(&edited("(\"barracks\", 1)", "(\"barracks\", 0)"), "count");
    refused(&edited("(\"rifleman\", 4)", "(\"rifleman\", 0)"), "weight");
    refused(&edited("name: \"normal\"", "name: \" \""), "name");
    refused(&edited("stagger: 3,", "stagger: 3, extra: 1,"), "extra");
}

#[test]
fn the_faction_sections_are_checked() {
    refused(
        &edited("faction: Veyran", "faction: Meridian"),
        "two sections",
    );
    refused(&edited("faction: Veyran", "faction: Neutral"), "Neutral");
}

/// The shipped profile with the Meridian build order and mix replaced.
fn meridian(build: &[(&str, u16)], mix: &[(&str, u16)]) -> AiProfile {
    let mut p = AiProfile::embedded();
    p.factions[0].build_order = roles(build);
    p.factions[0].unit_mix = roles(mix);
    p
}

fn refused_check(p: &AiProfile, what: &str) {
    let e = p.check(&UnitDefs::embedded()).unwrap_err().to_string();
    assert!(e.contains(what), "{e:?} should mention {what:?}");
}

const BASE: [(&str, u16); 3] = [("power_plant", 1), ("refinery", 1), ("barracks", 1)];

#[test]
fn a_small_profile_checks() {
    meridian(&BASE, &[("rifleman", 1)])
        .check(&UnitDefs::embedded())
        .unwrap();
}

#[test]
fn the_roles_are_checked_against_the_definitions() {
    let mix = [("rifleman", 1)];
    let with = |extra: (&str, u16)| {
        let mut b = BASE.to_vec();
        b.push(extra);
        meridian(&b, &mix)
    };
    refused_check(&with(("no_such_role", 1)), "no such role");
    // Open to Veyran only.
    refused_check(&with(("fortified_bunker", 1)), "not open to Meridian");
    refused_check(&with(("rifleman", 1)), "not a structure");
    // The base anchor cannot be built.
    refused_check(&with(("construction_center", 1)), "cannot be built");
    refused_check(&meridian(&BASE, &[("no_such_role", 1)]), "no such role");
    refused_check(
        &meridian(&BASE, &[("sniper", 1), ("sniper", 2)]),
        "listed twice",
    );
    refused_check(&meridian(&BASE, &[("medic", 1)]), "not open to Meridian");
    refused_check(
        &meridian(&BASE, &[("power_plant", 1)]),
        "only infantry and vehicles",
    );
    refused_check(&meridian(&BASE, &[("harvester", 1)]), "harvesters");
}

#[test]
fn a_prerequisite_must_be_earlier_in_the_list() {
    let mix = [("rifleman", 1)];
    // The tower needs the barracks, which stands first in BASE; before it, it is refused.
    refused_check(
        &meridian(
            &[("power_plant", 1), ("defence_tower", 1), ("barracks", 1)],
            &mix,
        ),
        "prerequisite \"barracks\"",
    );
    // The advanced defences need the tower and the radar.
    let mut b = BASE.to_vec();
    b.extend([("defence_tower", 1), ("advanced_defences", 1)]);
    refused_check(&meridian(&b, &mix), "prerequisite \"radar\"");
}

#[test]
fn a_repeated_role_asks_for_more_each_time() {
    let mut b = BASE.to_vec();
    b.push(("power_plant", 1));
    refused_check(
        &meridian(&b, &[("rifleman", 1)]),
        "which must be above the 1 before",
    );
}

#[test]
fn a_unit_needs_its_factory_and_what_it_requires_in_the_list() {
    // The light tank is built by a vehicle factory, which BASE does not have.
    refused_check(
        &meridian(&BASE, &[("light_tank", 1)]),
        "VehicleFactory missing",
    );
    let mut b = BASE.to_vec();
    b.push(("vehicle_factory", 1));
    meridian(&b, &[("light_tank", 1)])
        .check(&UnitDefs::embedded())
        .unwrap();
}
