//! What a structure is for (WP6.1): its function, the side units use and the
//! buildings it needs.
//!
//! A structure's footprint, price, build time, power and factions stay where
//! they were, in the record of `unit_defs.ron` (one place for each number,
//! and the numbers of `kind` are shared with the units). The optional
//! `building: (...)` block of a Structure adds what the later work packages key
//! on: the [`BuildingFunction`] (production, harvester dock, radar, defence,
//! ...), the [`Side`] on which units enter or leave, and `requires`, the
//! prerequisite buildings of the tech tree. `docs/rules/buildings.md` has the
//! field meanings and the shipped tree.
//!
//! A record without the block is a building with no function, no side and no
//! prerequisites; that is also what the frozen copies of the definitions in
//! `assets/scenarios/*_defs` mean, so they load unchanged.

use serde::Deserialize;

use super::error::{DefError, Source};
use super::unit::{Category, UnitDef};

/// Role ids that are never used: the original game's names for the base
/// anchor and the naval factory. `construction_center` and `shipyard` are the
/// names of this game (`content/units.md`).
pub const BANNED_IDS: [&str; 2] = ["construction_yard", "naval_yard"];

/// What a structure does for its owner. The simulation keys on this, not on
/// the id. The numbers are permanent (they are hashed and may be saved); a new
/// function takes the next free number.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum BuildingFunction {
    /// Nothing the rules act on (a mission structure, a storage tank).
    #[default]
    None,
    /// The base anchor: the owner builds around it (`build_radius`).
    Anchor,
    /// Produces power (`power` above 0).
    PowerPlant,
    /// Harvesters unload here, on the `exit_side`.
    HarvesterDock,
    /// Produces infantry.
    InfantryFactory,
    /// Produces vehicles.
    VehicleFactory,
    /// Produces vessels; stands on the coast (`needs_water`).
    VesselFactory,
    /// Enables the minimap and reveals its surroundings (`minimap`).
    Radar,
    /// Fights or shelters: a tower, an emplacement, a bunker.
    Defence,
    /// Looks like another building to the enemy AI and draws fire.
    Decoy,
}

impl BuildingFunction {
    /// The permanent number of the function.
    pub const fn number(self) -> u8 {
        match self {
            BuildingFunction::None => 0,
            BuildingFunction::Anchor => 1,
            BuildingFunction::PowerPlant => 2,
            BuildingFunction::HarvesterDock => 3,
            BuildingFunction::InfantryFactory => 4,
            BuildingFunction::VehicleFactory => 5,
            BuildingFunction::VesselFactory => 6,
            BuildingFunction::Radar => 7,
            BuildingFunction::Defence => 8,
            BuildingFunction::Decoy => 9,
        }
    }

    /// Whether units come out of it: they need a side to appear on.
    pub const fn is_factory(self) -> bool {
        matches!(
            self,
            BuildingFunction::InfantryFactory
                | BuildingFunction::VehicleFactory
                | BuildingFunction::VesselFactory
        )
    }

    /// Whether units go in or out of it, so it needs an `exit_side`: a
    /// factory or the harvester dock.
    pub const fn needs_exit(self) -> bool {
        self.is_factory() || matches!(self, BuildingFunction::HarvesterDock)
    }
}

/// A side of a footprint. The cell grid has +x to the east and +y to the south
/// (a footprint is anchored at its smallest x and y and extends east and
/// south), and the front of a model points along +x (`docs/conventions.md`),
/// so `PosX` is the side the art faces.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Side {
    /// The east side, +x: the side the models face.
    PosX,
    /// The west side, -x.
    NegX,
    /// The south side, +y.
    PosY,
    /// The north side, -y.
    NegY,
}

impl Side {
    /// The permanent number of the side.
    pub const fn number(self) -> u8 {
        match self {
            Side::PosX => 0,
            Side::NegX => 1,
            Side::PosY => 2,
            Side::NegY => 3,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Side::PosX => "PosX",
            Side::NegX => "NegX",
            Side::PosY => "PosY",
            Side::NegY => "NegY",
        }
    }
}

/// The `building` block of a Structure record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BuildingDef {
    /// What it does. Default: nothing the rules act on.
    pub function: BuildingFunction,
    /// The side of the footprint where units come out (a factory) or go in (the
    /// harvester dock). Required for those, refused for any other function.
    pub exit_side: Option<Side>,
    /// Ids of the structures its owner must have standing before this one can
    /// be built. Besides these, anything buildable needs the owner's
    /// construction_center (placement, WP6.2). Only for a buildable structure
    /// (one with a cost); each must be a Structure that is buildable or the
    /// base anchor, and available to every faction this one is.
    pub requires: Vec<String>,
    /// Units of ore this structure stores for its owner while it stands (WP7.3,
    /// `docs/rules/economy.md`, "Storage"). Default 0: it stores nothing. At most
    /// [`MAX_STORAGE`].
    pub storage: u32,
}

/// The largest `storage` a structure may have: a hundred full harvester loads of today's data,
/// far above any placeholder, and small enough that a player's sum never comes near `u64`.
pub const MAX_STORAGE: u32 = 1_000_000;

/// The function of the factory that builds a unit of `category` (WP6.5): infantry come out of
/// the infantry factory, vehicles out of the vehicle factory, vessels out of the vessel factory.
/// `None` for a structure and an object, which no factory builds. Derived rather than written in
/// each record: the category already says it, and a second field could only disagree.
pub const fn produced_by(category: Category) -> Option<BuildingFunction> {
    match category {
        Category::Infantry => Some(BuildingFunction::InfantryFactory),
        Category::Vehicle => Some(BuildingFunction::VehicleFactory),
        Category::Vessel => Some(BuildingFunction::VesselFactory),
        Category::Structure | Category::Object => None,
    }
}

/// The checks that make a unit producible (WP6.5): its `requires` list, and that a factory of the
/// right function exists for every faction that may have it.
pub(crate) fn check_unit_production(
    src: Source<'_>,
    units: &[UnitDef],
    u: &UnitDef,
) -> Result<(), DefError> {
    let at = |needle: &str, msg: String| {
        src.error_in_record("id", &u.id, needle, format!("unit \"{}\": {msg}", u.id))
    };
    if u.category == Category::Structure && !u.requires.is_empty() {
        return Err(at(
            "requires",
            "requires on a Structure: say it in the building block".into(),
        ));
    }
    if !u.requires.is_empty() && u.cost.is_none() {
        return Err(at(
            &u.requires[0],
            "requires on a unit that cannot be built (no cost)".into(),
        ));
    }
    for (i, r) in u.requires.iter().enumerate() {
        if u.requires[..i].contains(r) {
            return Err(at(r, format!("requires: \"{r}\" is listed twice")));
        }
        let Some(p) = units.iter().find(|d| d.id == *r) else {
            return Err(at(
                r,
                format!("requires: unknown prerequisite \"{r}\" (not in unit_defs.ron)"),
            ));
        };
        if p.category != Category::Structure {
            return Err(at(r, format!("requires: \"{r}\" is not a Structure")));
        }
        let anchor = p
            .building
            .as_ref()
            .is_some_and(|pb| pb.function == BuildingFunction::Anchor);
        if p.cost.is_none() && !anchor {
            return Err(at(
                r,
                format!("requires: \"{r}\" can never be built (no cost) and is not the anchor"),
            ));
        }
        if let Some(missing) = u.factions.iter().find(|fa| !p.factions.contains(fa)) {
            return Err(at(
                r,
                format!("requires: \"{r}\" is not available to {missing:?}, which this unit is"),
            ));
        }
    }
    // A unit with a price must be buildable: a factory of its function for each of its factions.
    // Only in a file that has factories at all: the frozen copies of the early scenarios have no
    // building blocks and so no factories, and are not production worlds.
    let has_factories = units
        .iter()
        .any(|f| f.building.as_ref().is_some_and(|b| b.function.is_factory()));
    if has_factories
        && u.cost.is_some()
        && let Some(function) = produced_by(u.category)
    {
        for faction in &u.factions {
            let has_factory = units.iter().any(|f| {
                f.cost.is_some()
                    && f.factions.contains(faction)
                    && f.building.as_ref().is_some_and(|b| b.function == function)
            });
            if !has_factory {
                return Err(at(
                    &u.id,
                    format!("no buildable {function:?} for {faction:?}: nothing could build it"),
                ));
            }
        }
    }
    Ok(())
}

/// The checks of one record's `building` block (everything but the cycle).
pub(crate) fn check_record(
    src: Source<'_>,
    units: &[UnitDef],
    u: &UnitDef,
) -> Result<(), DefError> {
    let at = |needle: &str, msg: String| {
        src.error_in_record("id", &u.id, needle, format!("unit \"{}\": {msg}", u.id))
    };
    let Some(b) = &u.building else {
        return Ok(());
    };
    if u.category != Category::Structure {
        return Err(at(&u.id, "only a Structure has a building block".into()));
    }
    let f = b.function;
    match (b.exit_side, f.needs_exit()) {
        (None, true) => {
            return Err(at(
                &u.id,
                format!("function {f:?} needs an exit_side (the side units come out or go in)"),
            ));
        }
        (Some(s), false) => {
            return Err(at(
                s.name(),
                format!("exit_side {} on a building with function {f:?}", s.name()),
            ));
        }
        _ => {}
    }
    if b.storage > MAX_STORAGE {
        return Err(at(
            "storage",
            format!("storage {} is above the maximum {MAX_STORAGE}", b.storage),
        ));
    }
    let bad = |what: &str| at(&u.id, format!("function {f:?} needs {what}"));
    match f {
        BuildingFunction::PowerPlant if u.power <= 0 => return Err(bad("power above 0")),
        BuildingFunction::Anchor if u.flags.build_radius.milli() <= 0 => {
            return Err(bad("a build_radius"));
        }
        BuildingFunction::Radar if !u.flags.minimap => return Err(bad("the minimap flag")),
        BuildingFunction::VesselFactory if !u.flags.needs_water => {
            return Err(bad("the needs_water flag"));
        }
        BuildingFunction::Decoy if u.flags.disguise_as.is_none() => {
            return Err(bad("a disguise_as"));
        }
        _ => {}
    }
    if !b.requires.is_empty() && u.cost.is_none() {
        return Err(at(
            &b.requires[0],
            "requires on a structure that cannot be built (no cost)".into(),
        ));
    }
    for (i, r) in b.requires.iter().enumerate() {
        if b.requires[..i].contains(r) {
            return Err(at(r, format!("requires: \"{r}\" is listed twice")));
        }
        let Some(p) = units.iter().find(|d| d.id == *r) else {
            return Err(at(
                r,
                format!("requires: unknown prerequisite \"{r}\" (not in unit_defs.ron)"),
            ));
        };
        if p.category != Category::Structure {
            return Err(at(r, format!("requires: \"{r}\" is not a Structure")));
        }
        if p.id == u.id {
            return Err(src.error_in_record_after(
                "id",
                &u.id,
                "requires:",
                r,
                format!("unit \"{}\": requires itself", u.id),
            ));
        }
        let anchor = p
            .building
            .as_ref()
            .is_some_and(|pb| pb.function == BuildingFunction::Anchor);
        if p.cost.is_none() && !anchor {
            return Err(at(
                r,
                format!("requires: \"{r}\" can never be built (no cost) and is not the anchor"),
            ));
        }
        if let Some(missing) = u.factions.iter().find(|fa| !p.factions.contains(fa)) {
            return Err(at(
                r,
                format!(
                    "requires: \"{r}\" is not available to {missing:?}, which this structure is"
                ),
            ));
        }
    }
    Ok(())
}

/// Depth-first walk of the `requires` graph; the cycle found, as indices into
/// `order`, first node repeated at the end.
fn visit(
    i: usize,
    order: &[&UnitDef],
    state: &mut [u8],
    path: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    state[i] = 1;
    path.push(i);
    let requires = order[i].building.as_ref().map_or(&[][..], |b| &b.requires);
    for r in requires {
        let Some(j) = order.iter().position(|u| u.id == *r) else {
            continue;
        };
        if state[j] == 1 {
            let from = path.iter().position(|&p| p == j).unwrap_or(0);
            let mut cycle = path[from..].to_vec();
            cycle.push(j);
            return Some(cycle);
        }
        if state[j] == 0
            && let Some(c) = visit(j, order, state, path)
        {
            return Some(c);
        }
    }
    path.pop();
    state[i] = 2;
    None
}

/// Refuses a cycle among the `requires` of the structures, naming it
/// (`a -> b -> a`). Depth-first in `kind` order, so the same file always
/// reports the same cycle.
pub(crate) fn check_cycles(src: Source<'_>, units: &[UnitDef]) -> Result<(), DefError> {
    let mut order: Vec<&UnitDef> = units.iter().filter(|u| u.building.is_some()).collect();
    order.sort_by_key(|u| u.kind);
    // 0 = not visited, 1 = on the current path, 2 = done.
    let mut state = vec![0u8; order.len()];
    let mut path: Vec<usize> = Vec::new();
    for i in 0..order.len() {
        if state[i] == 0
            && let Some(cycle) = visit(i, &order, &mut state, &mut path)
        {
            let names: Vec<&str> = cycle.iter().map(|&j| order[j].id.as_str()).collect();
            let first = names[0];
            return Err(src.error_in_record(
                "id",
                first,
                first,
                format!("tech tree cycle in requires: {}", names.join(" -> ")),
            ));
        }
    }
    Ok(())
}
