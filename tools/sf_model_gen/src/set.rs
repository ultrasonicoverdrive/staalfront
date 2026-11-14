//! Every model of set v5: the recipes of [`crate::models`] (WP11.6a and the units) and the
//! structures, their states, the mission objects and the rubble heaps of WP11.6c.

use crate::buildings::{self as b, F};
use crate::kit::Model;
use crate::models;
use crate::objects as o;
use crate::stages::{Stage, derive, derive_as};

/// A structure and its four states (`build_stages.ron`, `damage.ron`); `$slab` is the height of
/// its platform, which the scaffolding and the debris stand on.
macro_rules! with_states {
    ($v:ident, $b:expr, $slab:expr) => {
        $v.push(|| $b);
        $v.push(|| derive(&$b, Stage::Foundation, $slab));
        $v.push(|| derive(&$b, Stage::Scaffolding, $slab));
        $v.push(|| derive(&$b, Stage::Damaged, $slab));
        $v.push(|| derive(&$b, Stage::Heavy, $slab));
    };
}

/// The recipes of WP11.6c, in no particular order ([`all`] sorts).
pub fn structure_recipes() -> Vec<fn() -> Model> {
    let mut v: Vec<fn() -> Model> = Vec::new();
    // The three finished structures of WP11.6a get their states.
    v.push(|| derive(&models::meridian_construction_center(), Stage::Foundation, 500));
    v.push(|| derive(&models::meridian_construction_center(), Stage::Scaffolding, 500));
    v.push(|| derive(&models::meridian_construction_center(), Stage::Damaged, 500));
    v.push(|| derive(&models::meridian_construction_center(), Stage::Heavy, 500));
    v.push(|| derive(&models::veyran_power_plant(), Stage::Foundation, 400));
    v.push(|| derive(&models::veyran_power_plant(), Stage::Scaffolding, 400));
    v.push(|| derive(&models::veyran_power_plant(), Stage::Damaged, 400));
    v.push(|| derive(&models::veyran_power_plant(), Stage::Heavy, 400));
    v.push(|| derive(&models::veyran_defence_tower(), Stage::Foundation, 120));
    v.push(|| derive(&models::veyran_defence_tower(), Stage::Scaffolding, 120));
    v.push(|| derive(&models::veyran_defence_tower(), Stage::Damaged, 120));
    v.push(|| derive(&models::veyran_defence_tower(), Stage::Heavy, 120));
    // Meridian.
    with_states!(v, b::power_plant(F::M), 400);
    with_states!(v, b::refinery(F::M), 400);
    with_states!(v, b::barracks(F::M), 350);
    with_states!(v, b::vehicle_factory(F::M), 400);
    with_states!(v, b::radar(F::M), 300);
    with_states!(v, b::defence_tower(F::M), 300);
    with_states!(v, b::advanced_defences(F::M), 300);
    with_states!(v, b::shipyard(F::M), 500);
    with_states!(v, b::decoy_emplacement(F::M), 200);
    with_states!(v, b::ore_silo(F::M), 300);
    // Veyran.
    with_states!(v, b::construction_center(F::V), 450);
    with_states!(v, b::refinery(F::V), 400);
    with_states!(v, b::barracks(F::V), 350);
    with_states!(v, b::vehicle_factory(F::V), 400);
    with_states!(v, b::radar(F::V), 300);
    with_states!(v, b::advanced_defences(F::V), 300);
    with_states!(v, b::shipyard(F::V), 500);
    with_states!(v, b::fortified_bunker(F::V), 250);
    with_states!(v, b::ore_silo(F::V), 300);
    // Mission objects and their states (the names of set v4).
    v.push(o::relay_transmitter);
    v.push(|| derive(&o::relay_transmitter(), Stage::Damaged, 200));
    v.push(|| derive_as(&o::relay_transmitter(), Stage::Heavy, 200, "_rubble", 60));
    v.push(o::fuel_depot);
    v.push(|| derive(&o::fuel_depot(), Stage::Damaged, 200));
    v.push(|| derive_as(&o::fuel_depot(), Stage::Heavy, 200, "_burnt_out", 250));
    v.push(o::archive_building);
    v.push(|| derive(&o::archive_building(), Stage::Damaged, 200));
    v.push(|| derive(&o::archive_building(), Stage::Heavy, 200));
    v.push(o::bridge_segment);
    v.push(|| derive(&o::bridge_segment(), Stage::Damaged, 0));
    v.push(|| derive_as(&o::bridge_segment(), Stage::Heavy, 0, "_destroyed", 350));
    v.push(o::rail_spur);
    v.push(|| derive_as(&o::rail_spur(), Stage::Heavy, 0, "_destroyed", 400));
    v.push(|| o::generator(true));
    v.push(|| o::generator(false));
    v.push(|| derive_as(&o::generator(true), Stage::Heavy, 150, "_rubble", 150));
    v.push(o::evidence_crate);
    v.push(|| o::automated_gun(true));
    v.push(|| o::automated_gun(false));
    v.push(|| derive_as(&o::automated_gun(true), Stage::Heavy, 0, "_rubble", 150));
    v.push(|| o::unmanned_artillery(true));
    v.push(|| o::unmanned_artillery(false));
    v.push(|| derive_as(&o::unmanned_artillery(true), Stage::Heavy, 350, "_rubble", 150));
    // The rubble heaps, one per structure footprint (`wrecks.ron`).
    v.push(|| o::rubble(1, 1));
    v.push(|| o::rubble(2, 2));
    v.push(|| o::rubble(2, 3));
    v.push(|| o::rubble(3, 4));
    v.push(|| o::rubble(4, 4));
    v.push(|| o::rubble(4, 5));
    v
}

/// Every model of set v5, sorted by file name.
pub fn all() -> Vec<Model> {
    let mut out: Vec<Model> = models::MODELS.iter().map(|m| m()).collect();
    out.extend(structure_recipes().iter().map(|m| m()));
    out.sort_by(|a, b| a.stem.cmp(&b.stem));
    out
}
