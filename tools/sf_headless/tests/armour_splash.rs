//! The `armour_splash` scenario (WP5.3, `docs/rules/armour.md`): a hit takes the percentage of the armor
//! table, and a weapon with a splash radius hurts what is near where it goes off, less the farther out it
//! stands. Asserted along the scenario's groups, each against a hand calculation from the table of the frozen
//! definitions (`assets/scenarios/armour_splash_defs/`): bullets against heavy, light and no armor, rockets
//! against light and heavy vehicles and infantry, a sniper that does nothing to a tank, a howitzer shell
//! into a crowd (the falloff by hand, the armor of the bystanders, the shooter's own side, a structure), and
//! two bombs, one that hurts everyone and one that spares the shooter's side, one of them dropped on a
//! target so near that the shooter is in its own blast. The second half of the file runs the **shipped**
//! definitions (`assets/config`) and checks that the placeholder table does what its comments say. The
//! checksum of the scenario is pinned in `assets/scenarios/armour_splash.expected` (see `pinned.rs`).
//!
//! The cast and the commands are written in `assets/scenarios/armour_splash.ron`, which also lists the
//! entity ids used below.

mod common;

use common::scenario_path;
use sf_bridge::{unit_table, weapon_table};
use sf_data::units::UnitDefs;
use sf_headless::scenario::Scenario;
use sf_sim::armor::scaled_damage;
use sf_sim::{EntityId, Impact, ImpactCause, Shot, World};

const fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

// Group A.
const RA1: EntityId = id(0);
const TA1: EntityId = id(1);
const RA2: EntityId = id(2);
const VA2: EntityId = id(3);
const RA3: EntityId = id(4);
const TA3: EntityId = id(5);
// Group B.
const RB1: EntityId = id(6);
const TB1: EntityId = id(7);
const RB2: EntityId = id(8);
const TB2: EntityId = id(9);
const RB3: EntityId = id(10);
const VB3: EntityId = id(11);
// Group C.
const SC1: EntityId = id(12);
const TC1: EntityId = id(13);
const SC2: EntityId = id(14);
const VC2: EntityId = id(15);
// Group E.
const AE: EntityId = id(16);
const TE: EntityId = id(17);
const E1: EntityId = id(18);
const E2: EntityId = id(19);
const E3: EntityId = id(20);
const E4: EntityId = id(21);
const E5: EntityId = id(22);
const E6: EntityId = id(23);
// Group F.
const BF1: EntityId = id(24);
const TF1: EntityId = id(25);
const FF1: EntityId = id(26);
const FE1: EntityId = id(27);
const BF2: EntityId = id(28);
const TF2: EntityId = id(29);
const FF2: EntityId = id(30);
const FE2: EntityId = id(31);
// Group G.
const BG1: EntityId = id(32);
const VG1: EntityId = id(33);
const BG2: EntityId = id(34);
const VG2: EntityId = id(35);

const ENTITIES: usize = 36;

struct Run {
    world: World,
    shots: Vec<Shot>,
    impacts: Vec<Impact>,
    /// The hit points of every entity at the end of each tick: `hp[t][slot]`.
    hp: Vec<Vec<u32>>,
}

fn run() -> Run {
    let scenario = Scenario::load(&scenario_path("armour_splash.ron")).unwrap();
    let mut world = scenario.build_world().unwrap();
    let (mut shots, mut impacts, mut hp) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..scenario.ticks {
        world.step();
        shots.extend_from_slice(world.shots());
        impacts.extend_from_slice(world.impacts());
        hp.push(
            (0..ENTITIES as u32)
                .map(|s| world.hp_of(id(s)).unwrap())
                .collect(),
        );
    }
    Run {
        world,
        shots,
        impacts,
        hp,
    }
}

impl Run {
    fn hp(&self, e: EntityId) -> u32 {
        self.world.hp_of(e).expect("alive")
    }

    fn shots_by(&self, e: EntityId) -> Vec<Shot> {
        self.shots
            .iter()
            .copied()
            .filter(|s| s.attacker == e)
            .collect()
    }

    fn impacts_of(&self, e: EntityId) -> Vec<Impact> {
        self.impacts
            .iter()
            .copied()
            .filter(|i| i.owner == e)
            .collect()
    }

    /// How many hit points `e` lost on each tick it lost any: `(tick, lost)`.
    fn losses(&self, e: EntityId) -> Vec<(u64, u32)> {
        let slot = e.index() as usize;
        let mut last = self.hp[0][slot];
        let mut out = Vec::new();
        for (t, row) in self.hp.iter().enumerate() {
            if row[slot] != last {
                out.push((t as u64, last - row[slot]));
                last = row[slot];
            }
        }
        out
    }
}

// ---- group A: bullets -----------------------------------------------------------------------------------

#[test]
fn bullets_do_a_tenth_to_a_heavy_tank_two_fifths_to_a_light_one_and_all_to_a_rifleman() {
    let r = run();
    // Heavy armor takes 10 percent of 6, which is 3/5 and rounds to 1: a hit point a shot, never nothing.
    let shots = r.shots_by(RA1);
    assert!(shots.len() >= 25, "{}", shots.len());
    assert!(
        shots.iter().all(|s| s.damage == 6),
        "the shot reports the weapon's damage"
    );
    assert_eq!(r.hp(TA1), 900 - shots.len() as u32);
    assert!(r.losses(TA1).iter().all(|&(_, lost)| lost == 1));
    // Light armor takes 40 percent: 12/5, which rounds to 2.
    let shots = r.shots_by(RA3);
    assert_eq!(r.hp(TA3), 400 - 2 * shots.len() as u32);
    assert!(r.losses(TA3).iter().all(|&(_, lost)| lost == 2));
    // Infantry takes it all: 6 a shot, the 9th takes the rest of 50 and the order ends.
    let shots = r.shots_by(RA2);
    assert_eq!(shots.len(), 9, "a victim at zero is not shot at again");
    let lost: Vec<u32> = r.losses(VA2).iter().map(|&(_, l)| l).collect();
    assert_eq!(lost, [6, 6, 6, 6, 6, 6, 6, 6, 2], "50 = 8 * 6 + 2");
    assert_eq!(r.hp(VA2), 0);
}

// ---- group B: rockets -----------------------------------------------------------------------------------

#[test]
fn rockets_do_a_third_more_to_a_light_tank_the_full_45_to_a_heavy_one_and_two_fifths_to_infantry() {
    let r = run();
    // 130 percent of 45 is 58.5: a tie, rounded up to 59. Seven rockets: 6 * 59 = 354, the 7th finishes it.
    let hits = r.impacts_of(RB1);
    assert_eq!(hits.len(), 7);
    assert!(
        hits.iter().all(|i| i.victim == Some(TB1) && i.damage == 59),
        "{hits:?}"
    );
    assert!(
        hits.iter()
            .all(|i| i.cause == ImpactCause::Proximity && i.splash_victims == 0)
    );
    assert_eq!(r.hp(TB1), 0);
    assert_eq!(hits[5].hp_after, 400 - 6 * 59);
    // 100 percent against a heavy tank.
    let hits = r.impacts_of(RB2);
    assert!(hits.len() >= 10, "{}", hits.len());
    assert!(hits.iter().all(|i| i.victim == Some(TB2) && i.damage == 45));
    assert_eq!(r.hp(TB2), 900 - 45 * hits.len() as u32);
    // 40 percent against a rifleman: 18 a rocket, three rockets take 50.
    let hits = r.impacts_of(RB3);
    assert_eq!(hits.len(), 3);
    assert!(hits.iter().all(|i| i.damage == 18 && i.victim == Some(VB3)));
    assert_eq!(
        hits.iter().map(|i| i.hp_after).collect::<Vec<_>>(),
        [32, 14, 0]
    );
    assert_eq!(r.hp(VB3), 0);
}

// ---- group C: the sniper -------------------------------------------------------------------------------

#[test]
fn the_sniper_does_nothing_to_a_tank_and_two_fifths_of_a_rocket_throwers_hit_points() {
    let r = run();
    // 0 percent: the shots are fired and nothing comes off, and there is no minimum of one.
    let shots = r.shots_by(SC1);
    assert!(shots.len() >= 8, "{}", shots.len());
    assert!(shots.iter().all(|s| s.damage == 40 && s.hp_after == 900));
    assert_eq!(r.hp(TC1), 900);
    // 100 percent against a rocket thrower of 45 hit points: 40, then the rest.
    let shots = r.shots_by(SC2);
    assert_eq!(shots.iter().map(|s| s.hp_after).collect::<Vec<_>>(), [5, 0]);
    assert_eq!(r.hp(VC2), 0);
}

// ---- group E: a howitzer shell into a crowd --------------------------------------------------------------

#[test]
fn a_howitzer_shell_hurts_a_crowd_by_the_falloff_and_the_armor() {
    let r = run();
    let shells = r.impacts_of(AE);
    assert_eq!(
        shells.len(),
        1,
        "the target dies of the first shell and is not shot at again"
    );
    let shell = shells[0];
    assert_eq!(shell.cause, ImpactCause::Arrived);
    assert_eq!(shell.victim, Some(TE));
    assert_eq!(
        shell.damage, 90,
        "the direct hit: 100 percent of 90, once (not twice)"
    );
    // E1, E2, E3, E4 and the shooter's own rifleman E6; E5 is outside the radius.
    assert_eq!(shell.splash_victims, 5);
    // They were all hurt on the tick of the shell. The radius is 1.5 with full strength out to 0.5,
    // measured from the edge of a body.
    let tick = shell.tick.0;
    assert_eq!(r.hp(TE), 0, "the direct victim, 90 off 50");
    // E1, a rifleman a cell off (a radius of 0.2, so its edge is 0.8 away): 90 * (1.5 - 0.8) / 1 = 63.
    assert_eq!(r.losses(E1), [(tick, 50)], "63 off 50 hit points");
    assert_eq!(r.hp(E1), 0);
    // E2, a heavy tank two cells off (a radius of 0.75: edge 1.25): 90 * 50 percent * (1.5 - 1.25) = 11.25.
    assert_eq!(r.losses(E2), [(tick, 11)]);
    assert_eq!(r.hp(E2), 900 - 11);
    // E3, a light tank a cell off (a radius of 0.6: edge 0.4, inside the full-strength radius):
    // 90 * 75 percent = 67.5, a tie, rounded up.
    assert_eq!(r.losses(E3), [(tick, 68)]);
    assert_eq!(r.hp(E3), 400 - 68);
    // E4, a generator a cell off (a structure, a radius of 0.5: edge 0.5, still full strength):
    // 150 percent of 90.
    assert_eq!(r.losses(E4), [(tick, 135)]);
    assert_eq!(r.hp(E4), 400 - 135);
    // E5, two cells off, edge 1.8: outside.
    assert_eq!(r.hp(E5), 45);
    assert!(r.losses(E5).is_empty());
    // E6 is the shooter's own side and stands a cell from the target: friendly fire, 63 off 50.
    assert_eq!(r.losses(E6), [(tick, 50)]);
    // The howitzer itself is far away.
    assert_eq!(r.hp(AE), 300);
}

// ---- groups F and G: bombs ------------------------------------------------------------------------------

#[test]
fn a_bomb_hurts_the_shooters_side_unless_it_is_the_careful_one() {
    let r = run();
    // `bomb` (60, radius 2, full strength out to 1): the heavy tank takes 50 percent, 30. A friendly
    // rifleman a cell off (edge 0.8) takes the full 60 and dies; an enemy rifleman two cells off (edge
    // 1.8) takes 60 * 0.2 = 12.
    let bomb = r.impacts_of(BF1);
    assert_eq!(bomb.len(), 1);
    assert_eq!(bomb[0].victim, Some(TF1));
    assert_eq!(bomb[0].damage, 30);
    assert_eq!(bomb[0].splash_victims, 2);
    assert_eq!(r.hp(TF1), 900 - 30);
    assert_eq!(r.hp(FF1), 0, "the friend was in the blast");
    assert_eq!(r.hp(FE1), 50 - 12);
    assert_eq!(r.hp(BF1), 30, "the bomb team is far from its target");
    // `careful_bomb` (friendly_fire: false): the same blast, but the friend is spared.
    let bomb = r.impacts_of(BF2);
    assert_eq!(bomb.len(), 1);
    assert_eq!(bomb[0].damage, 30);
    assert_eq!(bomb[0].splash_victims, 1, "the enemy only");
    assert_eq!(r.hp(TF2), 900 - 30);
    assert_eq!(r.hp(FF2), 50, "spared");
    assert_eq!(r.hp(FE2), 50 - 12, "an enemy is hurt either way");
}

#[test]
fn a_bomb_dropped_next_to_its_team_takes_the_team_with_it_unless_it_is_careful() {
    let r = run();
    // The bomb team is a cell from its victim, so inside the full-strength radius of its own bomb.
    let bomb = r.impacts_of(BG1);
    assert_eq!(bomb.len(), 1);
    assert_eq!((bomb[0].victim, bomb[0].damage), (Some(VG1), 60));
    assert_eq!(bomb[0].splash_victims, 1, "the shooter");
    assert_eq!(r.hp(VG1), 0);
    assert_eq!(
        r.hp(BG1),
        0,
        "60 off 30 hit points: the shooter is no exception"
    );
    // The careful one spares itself, as it spares its side.
    let bomb = r.impacts_of(BG2);
    assert_eq!(bomb.len(), 1);
    assert_eq!((bomb[0].victim, bomb[0].damage), (Some(VG2), 60));
    assert_eq!(bomb[0].splash_victims, 0);
    assert_eq!(r.hp(VG2), 0);
    assert_eq!(r.hp(BG2), 30);
}

// ---- the whole run ----------------------------------------------------------------------------------------

#[test]
fn nothing_else_was_hurt_and_the_numbers_come_from_the_table() {
    let r = run();
    // Everything that is not named above is as it was.
    for (e, hp) in [
        (RA1, 50),
        (RA2, 50),
        (RA3, 50),
        (RB1, 45),
        (RB2, 45),
        (RB3, 45),
        (SC1, 40),
        (SC2, 40),
        (AE, 300),
        (BF2, 30),
    ] {
        assert_eq!(r.hp(e), hp, "{e:?}");
    }
    // The hand numbers are the table's: the same through the library function.
    let scenarios = scenario_path("");
    let defs =
        UnitDefs::load(&scenarios.join("armour_splash_defs"), &scenarios.join("..")).unwrap();
    let c = defs.classes();
    let pct = |damage: &str, armor: &str| {
        c.armor_percent(c.damage_kind(damage).unwrap(), c.armor_kind(armor).unwrap())
            .unwrap()
    };
    assert_eq!(scaled_damage(6, pct("bullet", "heavy"), 1, 1), 1);
    assert_eq!(scaled_damage(6, pct("bullet", "light"), 1, 1), 2);
    assert_eq!(scaled_damage(45, pct("rocket", "light"), 1, 1), 59);
    assert_eq!(scaled_damage(45, pct("rocket", "infantry"), 1, 1), 18);
    assert_eq!(scaled_damage(40, pct("precision", "heavy"), 1, 1), 0);
    assert_eq!(scaled_damage(90, pct("shell", "light"), 1, 1), 68);
    assert_eq!(scaled_damage(90, pct("shell", "structure"), 1, 1), 135);
    // 63 for E1 and 11 for E2, at the exact fractions of the falloff.
    assert_eq!(scaled_damage(90, pct("shell", "infantry"), 7, 10), 63);
    assert_eq!(scaled_damage(90, pct("shell", "heavy"), 1, 4), 11);
}

// ---- the shipped definitions -------------------------------------------------------------------------------

#[test]
fn the_shipped_placeholder_table_does_what_its_comments_say() {
    // Load the shipped set into a world and fire the shipped rifle, rocket, sniper and howitzer at a
    // heavy tank, a light tank, a rifleman and a structure, one weapon on one victim at a time, and
    // compare with the table.
    let defs = UnitDefs::embedded();
    let c = defs.classes();
    let pct = |damage: &str, armor: &str| {
        u32::from(
            c.armor_percent(c.damage_kind(damage).unwrap(), c.armor_kind(armor).unwrap())
                .unwrap(),
        )
    };
    let world = World::new(sf_sim::WorldConfig::new(1))
        .unwrap()
        .with_units(unit_table(&defs).unwrap())
        .unwrap()
        .with_weapons(weapon_table(&defs).unwrap())
        .unwrap();
    let armor = world.weapon_table().armor().expect("the shipped table");
    // Every weapon against every unit of the shipped set: what the world's table says is what the
    // data says.
    for w in defs.sim_weapons() {
        for role in defs.sim_units() {
            assert_eq!(
                armor.percent(w.damage_type, role.armor_type).map(u32::from),
                Some(pct(
                    &c.damage_types
                        .iter()
                        .find(|d| d.kind == w.damage_type)
                        .unwrap()
                        .name,
                    &c.armor_types
                        .iter()
                        .find(|a| a.kind == role.armor_type)
                        .unwrap()
                        .name
                ))
            );
        }
    }
    // The intent of the placeholders, in hit points a shot.
    let hit = |weapon: &str, armor_name: &str| {
        let w = defs.weapon(weapon).unwrap();
        scaled_damage(w.damage, pct(&w.damage_type, armor_name) as u16, 1, 1)
    };
    assert!(
        hit("rifle", "heavy") < hit("rifle", "light"),
        "rifles are poor against heavy armor"
    );
    assert!(hit("rifle", "light") < hit("rifle", "infantry"));
    assert!(
        hit("rocket", "light") > defs.weapon("rocket").unwrap().damage,
        "rockets are good against vehicles"
    );
    assert_eq!(
        hit("long_rifle", "heavy"),
        0,
        "the sniper hurts infantry only"
    );
    assert_eq!(hit("long_rifle", "infantry"), 40);
    assert!(hit("howitzer", "structure") > defs.weapon("howitzer").unwrap().damage);
    assert!(
        hit("satchel_charge", "structure") > 1200,
        "a satchel takes down the sturdiest structure"
    );
}
