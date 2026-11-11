//! Start sites: map knowledge the host hands to the AI.
//!
//! A human who knows the map knows where each player starts, so the AI is told the cell of every player's
//! construction centre from the match file (design memo, open choice 5). It is the only thing the AI learns
//! about the other players without seeing it; what stands there stays unknown until it is seen.

use sf_data::match_def::MatchDef;
use sf_data::units::{BuildingFunction, UnitDefs};
use sf_sim::{Cell, PlayerId};

/// The cell of the first construction centre (the base anchor) each player of `def` starts with, ascending
/// by player. A player of a checked match file always has one; a player without is left out.
pub fn start_sites(def: &MatchDef, defs: &UnitDefs) -> Vec<(PlayerId, Cell)> {
    let mut sites: Vec<(PlayerId, Cell)> = def
        .players
        .iter()
        .filter_map(|p| {
            p.objects
                .iter()
                .find(|o| {
                    defs.unit(&o.role).is_some_and(|d| {
                        d.building
                            .as_ref()
                            .is_some_and(|b| b.function == BuildingFunction::Anchor)
                    })
                })
                .map(|o| (PlayerId(p.id), Cell::new(o.cell.0, o.cell.1)))
        })
        .collect();
    sites.sort();
    sites
}
