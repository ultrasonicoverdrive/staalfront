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
        (20, 2, 300, 1, 90, 70, 600, 32)
    );
    // WP9.1b-1: the lane and the aprons, written out in the file and equal to the defaults of a profile that
    // leaves them out.
    assert_eq!(
        (e.lane, e.refinery_apron, e.factory_apron),
        (1, (3, 5), (1, 3))
    );
    assert_eq!(
        (DEFAULT_LANE, DEFAULT_REFINERY_APRON, DEFAULT_FACTORY_APRON),
        (1, (3, 5), (1, 3))
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
        (3000, 14_000, 4, 2400, 400, 10)
    );
    // WP9.1c-1: the numbers of production and the scout, written out in the file; the ones that equal the
    // default of a profile that leaves them out are checked against it in
    // `a_profile_without_the_numbers_of_wp9_1c_1_reads_with_the_defaults`.
    assert_eq!(e.max_queue, 2);
    assert_eq!(
        (
            a.gather_radius,
            a.gather_group,
            a.gather_tries,
            a.resend_ticks,
            a.scouts_max,
            a.scout_tick,
            a.scout_patience,
            a.scout_retry
        ),
        (8, 19, 3, 120, 2, 600, 1200, 3000)
    );
    // WP9.1c-2: the attack numbers and the army floor.
    assert_eq!(
        (a.arrive_radius, a.attack_patience, a.army_floor_value),
        (6, 2400, 2000)
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
            ("vehicle_factory", 1),
            ("barracks", 1),
            ("power_plant", 2),
            ("defence_tower", 1),
            ("refinery", 2),
            ("radar", 1),
            ("defence_tower", 2),
            ("advanced_defences", 1),
        ])
    );
    assert_eq!(meridian.unit_mix, mix);
    assert_eq!(meridian.scout.as_deref(), Some("scout"));
    let veyran = &p.factions[1];
    assert_eq!(veyran.faction, Faction::Veyran);
    assert_eq!(
        veyran.build_order,
        roles(&[
            ("power_plant", 1),
            ("refinery", 1),
            ("vehicle_factory", 1),
            ("barracks", 1),
            ("power_plant", 2),
            ("defence_tower", 1),
            ("fortified_bunker", 1),
            ("refinery", 2),
            ("radar", 1),
            ("defence_tower", 2),
            ("advanced_defences", 1),
        ])
    );
    assert_eq!(veyran.unit_mix, mix);
    assert_eq!(veyran.scout.as_deref(), Some("scout"));
}

#[test]
fn the_shipped_profile_fits_the_shipped_definitions() {
    AiProfile::embedded().check(&UnitDefs::embedded()).unwrap();
}

#[test]
fn every_number_is_checked_at_both_ends_when_the_file_is_read() {
    // (the text in the shipped file, the name the error mentions, the values just outside the range).
    let table: [(&str, &str, [&str; 2]); 33] = [
        ("lane: 1", "economy.lane", ["5", "5"]),
        (
            "refinery_apron: (3, 5)",
            "refinery_apron",
            ["(3, 0)", "(11, 5)"],
        ),
        (
            "factory_apron: (1, 3)",
            "factory_apron",
            ["(2, 3)", "(1, 10)"],
        ),
        ("think_interval: 10", "think_interval", ["0", "601"]),
        ("power_margin: 20", "power_margin", ["1001", "1001"]),
        (
            "harvesters_per_refinery: 2",
            "harvesters_per_refinery",
            ["0", "9"],
        ),
        ("reserve: 300", "economy.reserve", ["1000001", "1000001"]),
        ("max_parallel_builds: 1", "max_parallel_builds", ["0", "5"]),
        ("silo_percent: 90", "silo_percent", ["0", "101"]),
        (
            "repair_below_percent: 70",
            "repair_below_percent",
            ["0", "101"],
        ),
        ("blocked_ticks: 600", "blocked_ticks", ["0", "72001"]),
        ("placement_cap: 32", "placement_cap", ["0", "65537"]),
        ("attack_value: 3000", "attack_value", ["0", "1000001"]),
        ("max_wait: 14000", "max_wait", ["0", "72001"]),
        ("min_units: 4", "min_units", ["0", "1001"]),
        (
            "first_attack_tick: 2400",
            "first_attack_tick",
            ["72001", "72001"],
        ),
        ("defend_ticks: 400", "defend_ticks", ["0", "72001"]),
        ("arrive_radius: 6", "arrive_radius", ["0", "33"]),
        ("attack_patience: 2400", "attack_patience", ["0", "72001"]),
        (
            "army_floor_value: 2000",
            "army_floor_value",
            ["1000001", "1000001"],
        ),
        ("gather_distance: 10", "gather_distance", ["0", "65"]),
        ("max_queue: 2", "economy.max_queue", ["0", "6"]),
        ("gather_radius: 8", "gather_radius", ["0", "33"]),
        ("gather_group: 19", "gather_group", ["0", "65"]),
        ("gather_tries: 3", "gather_tries", ["0", "11"]),
        ("resend_ticks: 120", "resend_ticks", ["0", "72001"]),
        ("scouts_max: 2", "scouts_max", ["5", "5"]),
        ("scout_tick: 600", "scout_tick", ["72001", "72001"]),
        ("scout_patience: 1200", "scout_patience", ["0", "72001"]),
        ("scout_retry: 3000", "scout_retry", ["0", "72001"]),
        (
            "(\"barracks\", 1)",
            "count",
            ["(\"barracks\", 0)", "(\"barracks\", 33)"],
        ),
        (
            "(\"rifleman\", 4)",
            "weight",
            ["(\"rifleman\", 0)", "(\"rifleman\", 1001)"],
        ),
        ("stagger: 3,", "stagger", ["stagger: 0,", "stagger: 5,"]),
    ];
    for (from, name, bad) in table {
        let key = from.split(':').next().unwrap();
        for value in bad {
            let to = if from.starts_with('(') || from.ends_with(',') {
                value.to_string()
            } else {
                format!("{key}: {value}")
            };
            refused(&edited(from, &to), name);
        }
    }
    // The limits themselves are fine.
    AiProfile::from_ron(&edited("think_interval: 10", "think_interval: 600")).unwrap();
    AiProfile::from_ron(&edited("reserve: 300", "reserve: 1000000")).unwrap();
    // A stagger that lets two of eight players think together: 10 / gcd(10, 5) = 2.
    refused(&edited("stagger: 3", "stagger: 5"), "stagger");
    refused(&edited("name: \"normal\"", "name: \" \""), "name");
    refused(&edited("stagger: 3,", "stagger: 3, extra: 1,"), "extra");
}

#[test]
fn a_profile_without_the_base_numbers_of_wp9_1b_reads_with_the_defaults() {
    // The frozen copies of earlier scenarios (ai_fog_defs/normal.ron) were written before the lane and the
    // aprons existed and must keep reading.
    let old = EMBEDDED_NORMAL
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("lane:")
                || t.starts_with("refinery_apron:")
                || t.starts_with("factory_apron:"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let p = AiProfile::from_ron(&old).unwrap();
    assert_eq!(
        (
            p.economy.lane,
            p.economy.refinery_apron,
            p.economy.factory_apron
        ),
        (DEFAULT_LANE, DEFAULT_REFINERY_APRON, DEFAULT_FACTORY_APRON)
    );
    let frozen = include_str!("../../../../assets/scenarios/ai_fog_defs/normal.ron");
    assert_eq!(AiProfile::from_ron(frozen).unwrap().economy.lane, 1);
}

#[test]
fn a_profile_without_the_numbers_of_wp9_1c_1_reads_with_the_defaults() {
    // The frozen profile of ai_fog_defs was written before WP9.1c-1: no production or scout number, no scout.
    let frozen = include_str!("../../../../assets/scenarios/ai_fog_defs/normal.ron");
    let p = AiProfile::from_ron(frozen).unwrap();
    assert_eq!(p.economy.max_queue, DEFAULT_MAX_QUEUE);
    let a = p.army;
    assert_eq!(
        (
            a.gather_radius,
            a.gather_group,
            a.gather_tries,
            a.resend_ticks,
            a.scouts_max,
            a.scout_tick,
            a.scout_patience,
            a.scout_retry
        ),
        (
            DEFAULT_GATHER_RADIUS,
            DEFAULT_GATHER_GROUP,
            DEFAULT_GATHER_TRIES,
            DEFAULT_RESEND_TICKS,
            DEFAULT_SCOUTS_MAX,
            DEFAULT_SCOUT_TICK,
            DEFAULT_SCOUT_PATIENCE,
            DEFAULT_SCOUT_RETRY
        )
    );
    // The defaults as literals.
    assert_eq!(
        (
            DEFAULT_MAX_QUEUE,
            DEFAULT_GATHER_RADIUS,
            DEFAULT_GATHER_GROUP,
            DEFAULT_GATHER_TRIES
        ),
        (2, 8, 19, 3)
    );
    assert_eq!(
        (
            DEFAULT_RESEND_TICKS,
            DEFAULT_SCOUTS_MAX,
            DEFAULT_SCOUT_TICK,
            DEFAULT_SCOUT_PATIENCE,
            DEFAULT_SCOUT_RETRY
        ),
        (120, 2, 600, 1200, 3000)
    );
    assert!(p.factions.iter().all(|f| f.scout.is_none()));
    // WP9.1c-2: the attack numbers and the floor read with their defaults too (no floor).
    assert_eq!(
        (a.arrive_radius, a.attack_patience, a.army_floor_value),
        (
            DEFAULT_ARRIVE_RADIUS,
            DEFAULT_ATTACK_PATIENCE,
            DEFAULT_ARMY_FLOOR_VALUE
        )
    );
    assert_eq!(
        (
            DEFAULT_ARRIVE_RADIUS,
            DEFAULT_ATTACK_PATIENCE,
            DEFAULT_ARMY_FLOOR_VALUE
        ),
        (6, 2400, 0)
    );
    // The frozen profiles of the WP9.1b scenarios have no unit mix and no scout.
    for name in [
        "ai_base",
        "ai_economy",
        "ai_rebuild",
        "ai_deadlock",
        "ai_fog",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../assets/scenarios/{name}_defs/normal.ron"));
        let p = AiProfile::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(
            p.factions
                .iter()
                .all(|f| f.unit_mix.is_empty() && f.scout.is_none()),
            "{name}"
        );
    }
}

#[test]
fn an_empty_unit_mix_is_a_profile_that_makes_no_units() {
    meridian(&BASE, &[]).check(&UnitDefs::embedded()).unwrap();
    // The shipped text with line endings made `\n` (a Windows checkout has `\r\n`), the Meridian mix and scout
    // cut out.
    let shipped = EMBEDDED_NORMAL.replace("\r\n", "\n");
    let from = "unit_mix: [(\"rifleman\", 4), (\"rocket_trooper\", 2), (\"light_tank\", 3)],\n            scout: Some(\"scout\"),\n        ),\n        (\n            faction: Veyran";
    assert!(shipped.contains(from));
    let text = shipped.replacen(
        from,
        "unit_mix: [],\n        ),\n        (\n            faction: Veyran",
        1,
    );
    let p = AiProfile::from_ron(&text).unwrap();
    assert!(p.factions[0].unit_mix.is_empty() && p.factions[0].scout.is_none());
    p.check(&UnitDefs::embedded()).unwrap();
}

#[test]
fn the_scout_is_checked_like_a_unit_of_the_mix() {
    let with = |role: &str| {
        let mut p = meridian(&BASE, &[("rifleman", 1)]);
        p.factions[0].scout = Some(role.to_string());
        p
    };
    with("scout").check(&UnitDefs::embedded()).unwrap();
    refused_check(&with("no_such_role"), "scout (no_such_role): no such role");
    refused_check(&with("power_plant"), "only infantry and vehicles");
    refused_check(&with("harvester"), "harvesters");
    // The scout comes out of a barracks, which the list must have.
    let mut p = meridian(&[("power_plant", 1)], &[]);
    p.factions[0].scout = Some("scout".to_string());
    refused_check(&p, "InfantryFactory missing");
    // It may also be a role of the mix.
    let mut p = meridian(&BASE, &[("scout", 1)]);
    p.factions[0].scout = Some("scout".to_string());
    p.check(&UnitDefs::embedded()).unwrap();
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
