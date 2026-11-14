//! The profile a running AI uses: the file of `sf_data::ai_profile`, one faction's section, the roles turned
//! into kinds.

use sf_data::ai_profile::{AiProfile, ArmyRules, EconomyRules};
use sf_data::units::{Faction, UnitDefs};
use sf_math::hash::Fnv1a64;
use sf_sim::Kind;

/// One AI's profile, resolved for its faction. Plain data (it is part of the AI state a save keeps).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// The name of the file's profile.
    pub name: String,
    /// The faction the profile was resolved for (WP9.5a: the tournament checks that the AI plays the faction
    /// the world gave its seat). Not part of [`Profile::digest`]: the roles it turned into kinds are.
    pub faction: Faction,
    /// A think runs when `(tick + stagger * player) % think_interval == 0`.
    pub think_interval: u32,
    /// The offset per player.
    pub stagger: u32,
    /// The economy numbers.
    pub economy: EconomyRules,
    /// The army numbers.
    pub army: ArmyRules,
    /// The goal list: `(kind, count)`, "have at least `count`".
    pub build_order: Vec<(Kind, u16)>,
    /// `(kind, weight)`: the share of the army each kind aims at.
    pub unit_mix: Vec<(Kind, u16)>,
    /// WP9.1c-1: the kind of the scout; `None`: no scout.
    pub scout: Option<Kind>,
}

impl Profile {
    /// The profile of `def` for `faction`: the file is checked against `defs` first
    /// ([`AiProfile::check`]), then the roles of the faction's section become kinds. An error says what
    /// is wrong, including a profile with no section for the faction.
    pub fn resolve(def: &AiProfile, faction: Faction, defs: &UnitDefs) -> Result<Profile, String> {
        def.check(defs).map_err(|e| e.to_string())?;
        let plan = def
            .plan(faction)
            .ok_or_else(|| format!("profile \"{}\" has no section for {faction:?}", def.name))?;
        let kinds = |list: &[(String, u16)]| -> Result<Vec<(Kind, u16)>, String> {
            list.iter()
                .map(|(role, n)| {
                    defs.kind_of(role)
                        .map(|k| (Kind(k), *n))
                        .ok_or_else(|| format!("no role \"{role}\""))
                })
                .collect()
        };
        Ok(Profile {
            name: def.name.clone(),
            faction,
            think_interval: def.think_interval,
            stagger: def.stagger,
            economy: def.economy,
            army: def.army,
            build_order: kinds(&plan.build_order)?,
            unit_mix: kinds(&plan.unit_mix)?,
            scout: match &plan.scout {
                Some(role) => Some(
                    defs.kind_of(role)
                        .map(Kind)
                        .ok_or_else(|| format!("no role \"{role}\""))?,
                ),
                None => None,
            },
        })
    }

    /// Whether the production and army managers have anything to do: a unit mix or a scout. A profile with
    /// neither (the frozen profiles of the WP9.1b scenarios) leaves them off, so no rally point is set and no
    /// unit is ordered.
    pub fn makes_units(&self) -> bool {
        !self.unit_mix.is_empty() || self.scout.is_some()
    }

    /// A hash of everything in the profile.
    pub fn digest(&self) -> u64 {
        let mut h = Fnv1a64::new();
        h.write_bytes(self.name.as_bytes());
        h.write_u32(self.think_interval);
        h.write_u32(self.stagger);
        let e = &self.economy;
        h.write_u32(e.power_margin);
        h.write_u8(e.harvesters_per_refinery);
        h.write_u32(e.reserve);
        h.write_u8(e.max_parallel_builds);
        h.write_u8(e.silo_percent);
        h.write_u8(e.repair_below_percent);
        h.write_u32(e.blocked_ticks);
        h.write_u32(e.placement_cap);
        h.write_u8(e.lane);
        h.write_u8(e.max_queue);
        for (width, depth) in [e.refinery_apron, e.factory_apron] {
            h.write_u8(width);
            h.write_u8(depth);
        }
        let a = &self.army;
        h.write_u32(a.attack_value);
        h.write_u32(a.max_wait);
        h.write_u32(u32::from(a.min_units));
        h.write_u32(a.first_attack_tick);
        h.write_u32(a.defend_ticks);
        h.write_u8(a.gather_distance);
        h.write_u8(a.gather_radius);
        h.write_u8(a.gather_group);
        h.write_u8(a.gather_tries);
        h.write_u32(a.resend_ticks);
        h.write_u8(a.scouts_max);
        h.write_u32(a.scout_tick);
        h.write_u32(a.scout_patience);
        h.write_u32(a.scout_retry);
        // The numbers of WP9.1c-2
        h.write_u8(a.arrive_radius);
        h.write_u32(a.attack_patience);
        h.write_u32(a.army_floor_value);
        // The numbers of WP9.2a, only when one is set: a profile with all of them 0 keeps the digest it had.
        if a.groups_on() {
            h.write_u8(0x92);
            h.write_u32(u32::from(a.home_guard_permille));
            h.write_u8(a.stage_distance);
            h.write_u32(u32::from(a.stage_permille));
            h.write_u32(a.stage_patience);
            h.write_u32(a.reinforce_value);
            h.write_u32(a.defend_ratio_permille);
            h.write_u32(u32::from(a.timing_jitter_permille));
        }
        // The numbers of WP9.2a-2, only when one is set (a profile with both off keeps its digest).
        if a.gather_rules_on() {
            h.write_u8(0x93);
            h.write_u8(a.gather_refinery_gap);
            h.write_u8(u8::from(a.gather_everyone));
        }
        // The numbers of WP9.2b-1, only when the counter mix is on (off keeps the digest).
        if a.counter_on() {
            h.write_u8(0x94);
            h.write_u32(u32::from(a.counter_permille));
            h.write_u32(a.intel_ticks);
        }
        for list in [&self.build_order, &self.unit_mix] {
            h.write_u32(list.len() as u32);
            for (kind, n) in list {
                h.write_u32(u32::from(kind.0));
                h.write_u32(u32::from(*n));
            }
        }
        match self.scout {
            Some(kind) => {
                h.write_u8(1);
                h.write_u32(u32::from(kind.0));
            }
            None => h.write_u8(0),
        }
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(faction: Faction) -> Profile {
        Profile::resolve(&AiProfile::embedded(), faction, &UnitDefs::embedded()).unwrap()
    }

    #[test]
    fn the_roles_of_the_shipped_profile_become_the_kinds_of_the_shipped_definitions() {
        // Both sides of the mapping: the role names of assets/ai/normal.ron and the kind numbers of
        // unit_defs.ron (1 power_plant, 2 refinery, 3 barracks, 4 vehicle_factory, 5 radar,
        // 6 defence_tower, 7 advanced_defences, 10 fortified_bunker; 12 rifleman, 13 rocket_trooper,
        // 19 light_tank).
        let defs = UnitDefs::embedded();
        for (role, kind) in [
            ("power_plant", 1),
            ("refinery", 2),
            ("barracks", 3),
            ("vehicle_factory", 4),
            ("radar", 5),
            ("defence_tower", 6),
            ("advanced_defences", 7),
            ("fortified_bunker", 10),
            ("rifleman", 12),
            ("rocket_trooper", 13),
            ("light_tank", 19),
        ] {
            assert_eq!(defs.kind_of(role), Some(kind), "{role}");
        }
        let pairs = |list: &[(Kind, u16)]| -> Vec<(u16, u16)> {
            list.iter().map(|&(k, n)| (k.0, n)).collect()
        };
        let m = resolved(Faction::Meridian);
        assert_eq!(
            pairs(&m.build_order),
            [
                (1, 1),
                (2, 1),
                (4, 1),
                (3, 1),
                (1, 2),
                (2, 2),
                (6, 1),
                (5, 1),
                (6, 2),
                (7, 1)
            ]
        );
        assert_eq!(pairs(&m.unit_mix), [(12, 4), (13, 2), (19, 3)]);
        // Veyran adds the fortified bunker after the first tower (the second refinery comes before both).
        let v = resolved(Faction::Veyran);
        assert_eq!(pairs(&v.build_order)[5..8], [(2, 2), (6, 1), (10, 1)]);
        assert_eq!(v.build_order.len(), 11);
    }

    #[test]
    fn the_numbers_come_through() {
        let p = resolved(Faction::Meridian);
        assert_eq!(
            (p.name.as_str(), p.think_interval, p.stagger),
            ("normal", 10, 3)
        );
        assert_eq!((p.economy.reserve, p.army.attack_value), (300, 2000));
    }

    #[test]
    fn a_faction_without_a_section_is_an_error() {
        let mut def = AiProfile::embedded();
        def.factions.retain(|f| f.faction == Faction::Meridian);
        let e = Profile::resolve(&def, Faction::Veyran, &UnitDefs::embedded()).unwrap_err();
        assert!(e.contains("no section for Veyran"), "{e}");
    }

    #[test]
    fn a_profile_that_fails_the_check_does_not_resolve() {
        let mut def = AiProfile::embedded();
        def.factions[0].build_order[0].0 = "no_such_role".to_string();
        let e = Profile::resolve(&def, Faction::Meridian, &UnitDefs::embedded()).unwrap_err();
        assert!(e.contains("no such role"), "{e}");
    }

    #[test]
    fn a_profile_makes_units_when_it_has_a_mix_or_a_scout() {
        let base = resolved(Faction::Meridian);
        assert!(base.makes_units());
        let mut p = base.clone();
        p.scout = None;
        assert!(p.makes_units());
        p.unit_mix.clear();
        assert!(!p.makes_units());
        let mut p = base;
        p.unit_mix.clear();
        assert!(p.makes_units());
        // The scout of the shipped profile is kind 15 (unit_defs.ron).
        assert_eq!(p.scout, Some(Kind(15)));
    }

    #[test]
    fn the_digest_sees_every_number_and_list() {
        let base = resolved(Faction::Meridian);
        let digest = base.digest();
        assert_eq!(digest, resolved(Faction::Meridian).digest());
        assert_ne!(digest, resolved(Faction::Veyran).digest());
        let mut p = base.clone();
        p.economy.reserve += 1;
        assert_ne!(digest, p.digest());
        let mut p = base.clone();
        p.economy.lane += 1;
        assert_ne!(digest, p.digest());
        let mut p = base.clone();
        p.economy.refinery_apron.1 += 1;
        assert_ne!(digest, p.digest());
        let mut p = base.clone();
        p.economy.factory_apron.0 += 2;
        assert_ne!(digest, p.digest());
        let mut p = base.clone();
        p.army.gather_distance += 1;
        assert_ne!(digest, p.digest());
        // WP9.1c-1: every new number and the scout.
        let changes: [fn(&mut Profile); 27] = [
            |p| p.economy.max_queue += 1,
            |p| p.army.gather_radius += 1,
            |p| p.army.gather_group += 1,
            |p| p.army.gather_tries += 1,
            |p| p.army.resend_ticks += 1,
            |p| p.army.scouts_max += 1,
            |p| p.army.scout_tick += 1,
            |p| p.army.scout_patience += 1,
            |p| p.army.scout_retry += 1,
            |p| p.scout = None,
            // WP9.1c-2, and the numbers that act from it on.
            |p| p.army.arrive_radius += 1,
            |p| p.army.attack_patience += 1,
            |p| p.army.army_floor_value += 1,
            |p| p.army.attack_value += 1,
            |p| p.army.max_wait += 1,
            |p| p.army.min_units += 1,
            |p| p.army.first_attack_tick += 1,
            |p| p.army.defend_ticks += 1,
            // WP9.2a (the base profile is the shipped one, which has them on).
            |p| p.army.home_guard_permille += 1,
            |p| p.army.stage_distance += 1,
            |p| p.army.stage_permille += 1,
            |p| p.army.stage_patience += 1,
            |p| p.army.reinforce_value += 1,
            |p| p.army.defend_ratio_permille += 1,
            |p| p.army.timing_jitter_permille += 1,
            // WP9.2a-2, the gather rules.
            |p| p.army.gather_refinery_gap += 1,
            |p| p.army.gather_everyone = !p.army.gather_everyone,
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut p = base.clone();
            change(&mut p);
            assert_ne!(digest, p.digest(), "change {i}");
        }
        let mut p = base.clone();
        p.scout = Some(Kind(16));
        assert_ne!(digest, p.digest());
        let mut p = base.clone();
        p.build_order.swap(0, 1);
        assert_ne!(digest, p.digest());
        let mut p = base;
        p.unit_mix[0].1 += 1;
        assert_ne!(digest, p.digest());
    }

    #[test]
    fn the_counter_mix_numbers_are_hashed_only_when_it_is_on() {
        let base = resolved(Faction::Meridian);
        let mut off = base.clone();
        off.army.counter_permille = 0;
        let d = off.digest();
        // Off: the memory is not read and not hashed.
        off.army.intel_ticks += 1;
        assert_eq!(off.digest(), d);
        // On: the strength and the memory both move the digest, and so does switching it on.
        let mut on = base;
        on.army.counter_permille = 1000;
        let e = on.digest();
        assert_ne!(e, d);
        let mut p = on.clone();
        p.army.counter_permille += 1;
        assert_ne!(p.digest(), e);
        let mut p = on;
        p.army.intel_ticks += 1;
        assert_ne!(p.digest(), e);
    }
}
