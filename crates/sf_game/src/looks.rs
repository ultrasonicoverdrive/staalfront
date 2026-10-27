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
//!   strategic icon the definitions give it (the model of the owner's faction
//!   when the role has one per faction). Placeholder wanderers added with
//!   `--units` have no role and keep the population's mapping.

use sf_render::{Motion, Style, UnitId};
use sf_sim::{EntityView, WreckView};

use crate::population::Population;
use crate::roster::{Roster, faction_of_owner};

/// The player colour wrecks and rubble are drawn in: an entry after the four of the palette of
/// `assets/config/units.ron`, its sRGB the `colour` of `assets/config/wrecks.ron`, put there by [`with_wreck_colour`] (a model keeps its own materials; the team
/// parts and the primitives take it).
pub const WRECK_COLOUR: u8 = 4;

/// `style` with the wreck colour added at index [`WRECK_COLOUR`]. Every style the game draws units with goes
/// through it (the runner's placeholder style, the screenshot tests'). A palette that already has more than the
/// four player colours is left alone: the colour would not be at the index the wrecks use.
pub fn with_wreck_colour(style: Style) -> Style {
    if style.color_count() == usize::from(WRECK_COLOUR) {
        let (r, g, b) = crate::wreck_looks::WreckLooks::shipped().colour;
        style.with_colour("wreck", [r, g, b])
    } else {
        style
    }
}

/// The generation the picture gives the id of a wreck. It does not keep the wreck apart from the
/// entities: a live entity can have it too (`despawn` raises a slot to `u32::MAX` and still reuses it;
/// only the bump after that retires the slot). What does is the slot: [`Looks::wreck_motion`] is given
/// one past every live entity's, so a pick of a wreck names no entity. The value only makes a wreck's
/// id easy to recognise.
const WRECK_GENERATION: u32 = u32::MAX;

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
                    // The model of the owner's faction, if the role has one.
                    roster
                        .shape_for(v.kind, faction_of_owner(v.owner.0))
                        .unwrap_or(0)
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

    /// What a wreck or a pile of rubble is drawn as (WP5.5): the model or primitive of its class, in the
    /// wreck colour, where it lies. `slot` is an index no live entity has (the picture numbers its units
    /// by their slots): the wreck is not an entity, and a pick of it names none. `None` in the crowd,
    /// which has no wrecks, and for rubble of a footprint the roster has no look for.
    pub fn wreck_motion(&self, w: &WreckView, slot: u32) -> Option<Motion> {
        let Looks::Roster { roster, .. } = self else {
            return None;
        };
        let shape = roster.wreck_shape(w)?;
        Some(
            Motion::from_fixed(shape, WRECK_COLOUR, w.pos, w.pos, w.facing, w.facing)
                .with_id(UnitId::from_parts(slot, WRECK_GENERATION)),
        )
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
            turret: None,
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
