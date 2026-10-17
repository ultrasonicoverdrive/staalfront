//! How an entity of the simulation is drawn: which shape of the instancer, and
//! what the F1 panel calls it.
//!
//! The two scenes of the game shell have two ways to tell (WP3.2):
//!
//! * the **crowd** (the engine demo and its benchmark, `sf_game --crowd`) is
//!   made of placeholder wanderers whose kind is an opaque number;
//!   [`Population`] maps it to a shape, as the demo always did;
//! * the **sandbox** is made from the unit definitions: an entity the
//!   simulation made from its unit table (`EntityView::has_stats`) is a role,
//!   and the [`Roster`] maps its kind number to the model or primitive and the
//!   strategic icon the definitions give it. Placeholder wanderers added with
//!   `--units` have no role and keep the population's mapping.

use sf_sim::EntityView;

use crate::population::Population;
use crate::roster::Roster;

/// The mapping from entities to shapes of the scene being shown.
pub enum Looks {
    /// Opaque kinds, mapped by the demo's population.
    Crowd(Population),
    /// Roles of the unit definitions, with the crowd's mapping for entities
    /// that are not roles.
    Roster {
        /// The looks of the roles.
        roster: Box<Roster>,
        /// The mapping of the placeholder wanderers.
        crowd: Population,
    },
}

impl Looks {
    /// The shape (an index into the instancer's style) of an entity.
    pub fn shape_of(&self, v: &EntityView) -> u8 {
        match self {
            Looks::Crowd(population) => population.shape_of(v.kind),
            Looks::Roster { roster, crowd } => {
                if v.has_stats {
                    roster.shape_of(v.kind).unwrap_or(0)
                } else {
                    crowd.shape_of(v.kind)
                }
            }
        }
    }

    /// What an entity is called: the role id (`light_tank`), or
    /// `placeholder <kind>` for an entity that is not a role.
    pub fn name_of(&self, v: &EntityView) -> String {
        if let Looks::Roster { roster, .. } = self
            && v.has_stats
            && let Some(name) = roster.name_of(v.kind)
        {
            return name.to_owned();
        }
        format!("placeholder {}", v.kind.0)
    }

    /// The roster, in the sandbox.
    pub fn roster(&self) -> Option<&Roster> {
        match self {
            Looks::Roster { roster, .. } => Some(roster),
            Looks::Crowd(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_math::{Facing, Vec2};
    use sf_sim::{EntityId, Kind, PlayerId};

    fn view(kind: u16, has_stats: bool) -> EntityView {
        EntityView {
            id: EntityId::new(0, 0),
            kind: Kind(kind),
            owner: PlayerId(0),
            prev_pos: Vec2::ZERO,
            pos: Vec2::ZERO,
            prev_facing: Facing::default(),
            facing: Facing::default(),
            has_stats,
            hp: 0,
        }
    }

    #[test]
    fn the_crowd_maps_kinds_through_the_population() {
        let looks = Looks::Crowd(Population::new(None));
        assert_eq!(looks.shape_of(&view(2, false)), 2);
        assert_eq!(looks.name_of(&view(2, false)), "placeholder 2");
        assert!(looks.roster().is_none());
    }
}
