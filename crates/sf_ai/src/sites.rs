//! Start sites: map knowledge the host hands to the AI.
//!
//! A human who knows the map knows where each player starts, so the AI is told the cell of every player's
//! construction centre from the match file (design memo, open choice 5). It is the only thing the AI learns
//! about the other players without seeing it; what stands there stays unknown until it is seen.

use sf_data::ai_profile::AiProfile;
use sf_data::match_def::{Controller, MatchDef};
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

/// The computer opponents of a match (WP9.1d, moved here by WP9.5a so the game and `sf_headless tournament`
/// make them the same way), ascending by player id: one [`Ai`] for every player whose controller is
/// `Ai`, made by [`make_ai`] for the player's faction against `defs`, every one told the start sites of the
/// match ([`start_sites`]) and seeded with `seed`.
///
/// The crate reads no file: `profile_text(name)` is the host's way to get the text of the profile called
/// `name` (the game and the tournament read `<assets>/ai/<name>.ron`); its error is the whole message
/// ("cannot read ..."). Every error names the player and the profile.
pub fn match_ais(
    def: &MatchDef,
    defs: &UnitDefs,
    seed: u64,
    profile_text: &dyn Fn(&str) -> Result<String, String>,
) -> Result<Vec<Ai>, String> {
    let sites = start_sites(def, defs);
    let mut ais = Vec::new();
    for p in &def.players {
        let Controller::Ai { profile } = &p.controller else {
            continue;
        };
        let at = format!("player {} (ai profile \"{profile}\")", p.id);
        let text = profile_text(profile).map_err(|e| format!("{at}: {e}"))?;
        ais.push(
            make_ai(PlayerId(p.id), p.faction, &text, defs, seed, sites.clone())
                .map_err(|e| format!("{at}: {e}"))?,
        );
    }
    ais.sort_by_key(Ai::player);
    Ok(ais)
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

    const NORMAL: &str = include_str!("../../../assets/ai/normal.ron");

    fn duel() -> MatchDef {
        let mut def = MatchDef::embedded_ai();
        def.players[0].controller = Controller::Ai {
            profile: "normal".to_string(),
        };
        def
    }

    #[test]
    fn match_ais_makes_one_ai_per_ai_seat_ascending_and_seeds_each_with_the_seed() {
        let defs = UnitDefs::embedded();
        let read = |_: &str| Ok(NORMAL.to_string());
        // Standard seats only: none.
        assert!(
            match_ais(&MatchDef::embedded(), &defs, 7, &read)
                .unwrap()
                .is_empty()
        );
        let mut def = duel();
        def.players.reverse();
        let made = match_ais(&def, &defs, 7, &read).unwrap();
        assert_eq!(
            made.iter().map(|a| a.player().0).collect::<Vec<_>>(),
            [0, 1]
        );
        // The seed is part of the digest of each.
        let again = match_ais(&def, &defs, 7, &read).unwrap();
        let other = match_ais(&def, &defs, 8, &read).unwrap();
        assert_eq!(made, again);
        assert!(
            made.iter()
                .zip(&other)
                .all(|(a, b)| a.digest() != b.digest())
        );
    }

    #[test]
    fn match_ais_names_the_player_and_the_profile_when_the_host_cannot_read_it() {
        let defs = UnitDefs::embedded();
        let e = match_ais(&duel(), &defs, 7, &|name: &str| {
            Err(format!("cannot read {name}.ron: gone"))
        })
        .unwrap_err();
        assert!(
            e.contains("player 0") && e.contains("\"normal\"") && e.contains("gone"),
            "{e}"
        );
        // A text that is not a profile is named the same way.
        let e = match_ais(&duel(), &defs, 7, &|_: &str| Ok("(nope".to_string())).unwrap_err();
        assert!(e.contains("player 0") && e.contains("normal"), "{e}");
    }
}
