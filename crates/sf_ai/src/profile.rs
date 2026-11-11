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
            think_interval: def.think_interval,
            stagger: def.stagger,
            economy: def.economy,
            army: def.army,
            build_order: kinds(&plan.build_order)?,
            unit_mix: kinds(&plan.unit_mix)?,
        })
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
        let a = &self.army;
        h.write_u32(a.attack_value);
        h.write_u32(a.max_wait);
        h.write_u32(u32::from(a.min_units));
        h.write_u32(a.first_attack_tick);
        h.write_u32(a.defend_ticks);
        h.write_u8(a.gather_distance);
        for list in [&self.build_order, &self.unit_mix] {
            h.write_u32(list.len() as u32);
            for (kind, n) in list {
                h.write_u32(u32::from(kind.0));
                h.write_u32(u32::from(*n));
            }
        }
        h.finish()
    }
}
