//! Start sites: map knowledge the host hands to the AI.
//!
//! A human who knows the map knows where each player starts, so the AI is told the cell of every player's
//! construction centre from the match file (design memo, open choice 5). It is the only thing the AI learns
//! about the other players without seeing it; what stands there stays unknown until it is seen.

use sf_data::ai_profile::AiProfile;
use sf_data::match_def::MatchDef;
use sf_data::units::{BuildingFunction, Faction, UnitDefs};
use sf_sim::{Cell, PlayerId};

use crate::{Ai, Profile};

/// The one place an AI is made from the text of a profile file (WP9.1c-1, shared by `sf_headless`'s scenario
/// loader and the game's match loader): the text is read and checked ([`AiProfile::from_ron`]), resolved for
/// `faction` against `defs` ([`Profile::resolve`]) and the AI is made for `player` with `seed` and the
/// `sites` of the match ([`start_sites`]). The error says what is wrong with the profile; the host adds
/// which player and which file. The seed is the host's choice (a scenario's `ai` entry, a match's seed).
pub fn make_ai(
    player: PlayerId,
    faction: Faction,
    profile_text: &str,
    defs: &UnitDefs,
    seed: u64,
    sites: Vec<(PlayerId, Cell)>,
) -> Result<Ai, String> {
    let parsed = AiProfile::from_ron(profile_text).map_err(|e| e.to_string())?;
    let resolved = Profile::resolve(&parsed, faction, defs)?;
    Ok(Ai::new(player, resolved, seed, sites))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_sites_of_the_first_match_are_the_two_construction_centres() {
        let sites = start_sites(&MatchDef::embedded(), &UnitDefs::embedded());
        assert_eq!(
            sites,
            vec![
                (PlayerId(0), Cell::new(12, 68)),
                (PlayerId(1), Cell::new(72, 14))
            ]
        );
    }
}
