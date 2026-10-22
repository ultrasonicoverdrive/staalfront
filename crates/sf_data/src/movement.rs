//! Terrain costs per movement class (WP4.1): the one table that says which
//! movement class may enter which terrain type, and how fast.
//!
//! The data is split over two files because each half has its own owner: the
//! terrain types (`terrain_types.ron`, their ids are what map files store) and
//! the movement classes (`unit_classes.ron`, their `kind` numbers are what the
//! simulation, scenarios and saves store). The cost of a class on a type is
//! written with the class (`terrain_costs`), so adding a movement class is one
//! record and no Rust change. [`MovementCosts`] is the two files put together
//! and checked against each other:
//!
//! * every class has a cost for **every** terrain type (no silent default: a
//!   new terrain type has to be thought about for each class), and
//! * no class names a terrain type that does not exist (a typo would otherwise
//!   be a cost that nothing ever reads).
//!
//! Everything that needs "may this class enter this cell" reads this table:
//! the simulation through `sf_bridge` (`sf_sim::MovementTable`), and through
//! the simulation the cursor of `sf_game`, so there is one source of truth.
//!
//! ```
//! use sf_data::movement::MovementCosts;
//! use sf_data::units::TerrainCost;
//!
//! let costs = MovementCosts::embedded();
//! let tank = costs.class_by_name("tracked").unwrap();
//! assert_eq!(tank.cost_on_name("road"), Some(TerrainCost::Speed(1200)));
//! assert_eq!(tank.cost_on_name("water"), Some(TerrainCost::Impassable));
//! ```

use std::fmt;

use crate::terrain::TerrainTypes;
use crate::units::{ClassTables, TerrainCost, TurningDef, UnitDefs};

/// Why the movement classes and the terrain types do not fit together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MovementError {
    /// A class has no cost for a terrain type.
    MissingCost {
        /// The movement class.
        class: String,
        /// The terrain type it leaves out.
        terrain: String,
    },
    /// A class names a terrain type the table does not have.
    UnknownTerrain {
        /// The movement class.
        class: String,
        /// The name it wrote.
        terrain: String,
    },
}

impl fmt::Display for MovementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCost { class, terrain } => write!(
                f,
                "movement class \"{class}\": no cost for terrain \"{terrain}\" \
                 (every class needs one for every terrain type)"
            ),
            Self::UnknownTerrain { class, terrain } => write!(
                f,
                "movement class \"{class}\": there is no terrain type \"{terrain}\" \
                 (not in terrain_types.ron)"
            ),
        }
    }
}

impl std::error::Error for MovementError {}

/// The costs of one movement class on every terrain type, complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassCosts {
    /// The permanent `kind` number of the class.
    pub kind: u16,
    /// The class's name.
    pub name: String,
    /// Whether the class is a ground class (`ground` in `unit_classes.ron`).
    pub ground: bool,
    /// How the class turns its hull (`turning` in `unit_classes.ron`, WP4.5).
    pub turning: TurningDef,
    /// `(terrain type id, cost)`, one per terrain type, ascending by id.
    costs: Vec<(u8, TerrainCost)>,
    /// `(terrain type name, cost)` in the same order, for tools and tests.
    names: Vec<(String, TerrainCost)>,
}

impl ClassCosts {
    /// The cost on the terrain type with this id; `None` for an id the table
    /// does not have.
    pub fn cost_on(&self, terrain: u8) -> Option<TerrainCost> {
        self.costs
            .binary_search_by_key(&terrain, |&(id, _)| id)
            .ok()
            .map(|i| self.costs[i].1)
    }

    /// The cost on the terrain type with this name.
    pub fn cost_on_name(&self, terrain: &str) -> Option<TerrainCost> {
        self.names
            .iter()
            .find(|(name, _)| name == terrain)
            .map(|&(_, cost)| cost)
    }

    /// Every `(terrain type id, cost)`, ascending by id.
    pub fn costs(&self) -> &[(u8, TerrainCost)] {
        &self.costs
    }
}

/// The terrain costs of every movement class, checked against the terrain
/// types. Sorted by class `kind`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovementCosts {
    classes: Vec<ClassCosts>,
    /// Ids of the terrain types, ascending.
    terrain_ids: Vec<u8>,
}

impl MovementCosts {
    /// Puts the classes of `classes` together with the terrain `types` and
    /// checks that each fits the other.
    pub fn new(classes: &ClassTables, types: &TerrainTypes) -> Result<Self, MovementError> {
        let mut out = Vec::with_capacity(classes.movement_classes.len());
        for class in &classes.movement_classes {
            if let Some((terrain, _)) = class
                .terrain_costs
                .iter()
                .find(|(name, _)| types.id_of(name).is_none())
            {
                return Err(MovementError::UnknownTerrain {
                    class: class.name.clone(),
                    terrain: terrain.clone(),
                });
            }
            let mut costs = Vec::with_capacity(types.len());
            let mut names = Vec::with_capacity(types.len());
            for t in types.iter() {
                let cost = class
                    .cost_on(&t.name)
                    .ok_or_else(|| MovementError::MissingCost {
                        class: class.name.clone(),
                        terrain: t.name.clone(),
                    })?;
                costs.push((t.id, cost));
                names.push((t.name.clone(), cost));
            }
            out.push(ClassCosts {
                kind: class.kind,
                name: class.name.clone(),
                ground: class.ground,
                turning: class.turning,
                costs,
                names,
            });
        }
        out.sort_by_key(|c| c.kind);
        Ok(Self {
            classes: out,
            terrain_ids: types.iter().map(|t| t.id).collect(),
        })
    }

    /// The definitions' classes together with `types`.
    pub fn from_defs(defs: &UnitDefs, types: &TerrainTypes) -> Result<Self, MovementError> {
        Self::new(defs.classes(), types)
    }

    /// The shipped classes together with the shipped terrain types.
    ///
    /// # Panics
    ///
    /// If the shipped files do not fit each other, which a test rules out.
    pub fn embedded() -> Self {
        Self::from_defs(&UnitDefs::embedded(), &TerrainTypes::embedded())
            .unwrap_or_else(|e| panic!("the embedded movement costs are complete: {e}"))
    }

    /// Every class, ascending by `kind`.
    pub fn classes(&self) -> &[ClassCosts] {
        &self.classes
    }

    /// The class with this `kind` number.
    pub fn class(&self, kind: u16) -> Option<&ClassCosts> {
        self.classes
            .binary_search_by_key(&kind, |c| c.kind)
            .ok()
            .map(|i| &self.classes[i])
    }

    /// The class with this name.
    pub fn class_by_name(&self, name: &str) -> Option<&ClassCosts> {
        self.classes.iter().find(|c| c.name == name)
    }

    /// The cost of class `class` on terrain type `terrain`; `None` for a class
    /// or a terrain type the table does not have.
    pub fn cost(&self, class: u16, terrain: u8) -> Option<TerrainCost> {
        self.class(class).and_then(|c| c.cost_on(terrain))
    }

    /// Whether every ground class (those marked `ground: true` in
    /// `unit_classes.ron`; `foot`, `wheeled` and `tracked` as shipped) can enter
    /// terrain type `terrain`: open ground in the words of the early placement
    /// helpers, which the simulation still keeps as a summary flag of the
    /// terrain (`sf_sim::TerrainFlags::ground`, part of the terrain digest).
    /// Derived from the costs, never written by hand; true if no class is
    /// marked.
    pub fn ground_passable(&self, terrain: u8) -> bool {
        self.classes
            .iter()
            .filter(|c| c.ground)
            .all(|c| c.cost_on(terrain).is_some_and(TerrainCost::is_passable))
    }

    /// The ids of the terrain types the costs are complete for, ascending.
    pub fn terrain_ids(&self) -> &[u8] {
        &self.terrain_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::TerrainType;

    fn types(names: &[&str]) -> TerrainTypes {
        TerrainTypes::new(
            names
                .iter()
                .enumerate()
                .map(|(i, n)| TerrainType {
                    id: i as u8 * 2,
                    name: (*n).into(),
                    buildable: false,
                    harvestable: false,
                    minimap_colour: [200, 0, 200],
                })
                .collect(),
        )
        .unwrap()
    }

    fn classes(body: &str) -> ClassTables {
        let text = format!(
            "(movement_classes: [{body}], armor_types: [(kind: 0, name: \"none\")], \
             damage_types: [(kind: 0, name: \"bullet\")])"
        );
        ClassTables::from_ron("classes.ron", &text).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_complete_table_is_put_together_by_kind_and_terrain_id() {
        let c = classes(
            "(kind: 7, name: \"b\", terrain_costs: [(\"x\", Speed(900)), (\"y\", Impassable)]), \
             (kind: 3, name: \"a\", terrain_costs: [(\"y\", Speed(1000)), (\"x\", Speed(500))])",
        );
        let costs = MovementCosts::new(&c, &types(&["x", "y"])).unwrap();
        let kinds: Vec<u16> = costs.classes().iter().map(|c| c.kind).collect();
        assert_eq!(kinds, [3, 7]);
        assert_eq!(costs.terrain_ids(), [0, 2]);
        // The ids of the terrain table (0 and 2 here), whatever the file order.
        assert_eq!(costs.cost(3, 0), Some(TerrainCost::Speed(500)));
        assert_eq!(costs.cost(3, 2), Some(TerrainCost::Speed(1000)));
        assert_eq!(costs.cost(7, 2), Some(TerrainCost::Impassable));
        assert_eq!(costs.cost(7, 1), None, "no terrain type with id 1");
        assert_eq!(costs.cost(9, 0), None, "no class with kind 9");
        assert_eq!(
            costs.class_by_name("b").unwrap().cost_on_name("x"),
            Some(TerrainCost::Speed(900))
        );
    }

    #[test]
    fn a_missing_cost_and_an_unknown_terrain_are_refused() {
        let missing = classes("(kind: 0, name: \"a\", terrain_costs: [(\"x\", Speed(1000))])");
        let err = MovementCosts::new(&missing, &types(&["x", "y"])).unwrap_err();
        assert_eq!(
            err,
            MovementError::MissingCost {
                class: "a".into(),
                terrain: "y".into()
            }
        );
        assert!(
            err.to_string().contains("no cost for terrain \"y\""),
            "{err}"
        );

        let typo = classes(
            "(kind: 0, name: \"a\", terrain_costs: [(\"x\", Speed(1000)), (\"grasss\", Impassable)])",
        );
        let err = MovementCosts::new(&typo, &types(&["x"])).unwrap_err();
        assert_eq!(
            err,
            MovementError::UnknownTerrain {
                class: "a".into(),
                terrain: "grasss".into()
            }
        );
    }

    #[test]
    fn ground_passable_is_derived_from_the_classes_marked_ground() {
        let c = classes(
            "(kind: 0, name: \"a\", ground: true, terrain_costs: [(\"x\", Speed(1000)), (\"y\", Speed(500))]), \
             (kind: 1, name: \"b\", ground: true, terrain_costs: [(\"x\", Speed(1000)), (\"y\", Impassable)]), \
             (kind: 2, name: \"c\", terrain_costs: [(\"x\", Speed(1000)), (\"y\", Speed(1000))])",
        );
        let costs = MovementCosts::new(&c, &types(&["x", "y"])).unwrap();
        assert!(costs.ground_passable(0), "x: both ground classes enter");
        assert!(!costs.ground_passable(2), "y: class b cannot");
        assert!(!costs.ground_passable(9), "not a terrain type");
        // With no class marked there is no restriction known.
        let none = classes(
            "(kind: 0, name: \"a\", terrain_costs: [(\"x\", Impassable), (\"y\", Impassable)])",
        );
        let costs = MovementCosts::new(&none, &types(&["x", "y"])).unwrap();
        assert!(costs.ground_passable(0));
    }
}
