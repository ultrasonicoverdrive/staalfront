//! Choosing a structure to build and placing it (WP6.2): the part of the input that
//! has no rules in it.
//!
//! **B** cycles through the structures the local player's faction may build (the structures of
//! the unit definitions with a price and the faction among their `factions`, in kind order, then
//! back to none). With one chosen, the cursor carries a ghost ([`crate::placement_view`]) on the
//! cell under it, **left click** sends `Command::BuildStructure` for that cell and **right click**
//! puts the structure away. Selecting and ordering are off while a structure is chosen.
//!
//! Whether the place is valid is not decided here: [`BuildMode::ask`] tells the sim thread which
//! question to answer ([`GhostQuery`]) and the answer comes back in the snapshot. The click sends
//! the command whatever the ghost shows; the simulation checks it again at the tick it runs and
//! reports a refusal ([`crate::sim_thread::SimHandle::rejected_builds`]), so a ghost that was a
//! tick old cannot place anything the rules forbid.

use sf_data::units::{Category, Faction, UnitDefs};
use sf_sim::{Cell, Command, Kind, PlayerId};

use crate::sim_snapshot::GhostQuery;
use crate::sim_thread::SimHandle;

/// One structure the player may choose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The permanent kind number.
    pub kind: Kind,
    /// The role name (`barracks`).
    pub id: String,
    /// Width and height in cells, to centre the ghost on the cursor.
    pub size: (u8, u8),
}

/// The structure chosen to build and what the sim thread was last asked.
#[derive(Debug)]
pub struct BuildMode {
    choices: Vec<Choice>,
    /// The role name of every kind of the definitions, for "needs a power plant first".
    names: Vec<(Kind, String)>,
    /// Index into `choices`; none when no structure is chosen.
    chosen: Option<usize>,
    /// The question the sim thread is answering now.
    asked: Option<GhostQuery>,
    /// A cell that stands in for the cursor (`--build-at`, for screenshots).
    pinned: Option<Cell>,
}

impl BuildMode {
    /// The structures `faction` may build in `defs`: those with a price whose `factions` name it,
    /// by kind. The anchor and the mission structures have no price and are not offered.
    pub fn for_faction(defs: &UnitDefs, faction: Faction) -> BuildMode {
        let mut choices: Vec<Choice> = defs
            .units()
            .iter()
            .filter(|d| {
                d.category == Category::Structure
                    && d.cost.is_some()
                    && d.factions.contains(&faction)
            })
            .map(|d| Choice {
                kind: Kind(d.kind),
                id: d.id.clone(),
                size: defs
                    .sim_unit(d.kind)
                    .and_then(|u| u.footprint)
                    .unwrap_or((1, 1)),
            })
            .collect();
        choices.sort_by_key(|c| c.kind);
        let names = defs
            .units()
            .iter()
            .map(|d| (Kind(d.kind), d.id.clone()))
            .collect();
        BuildMode {
            choices,
            names,
            chosen: None,
            asked: None,
            pinned: None,
        }
    }

    /// No structures on offer: the scenes without a build menu (the crowd).
    pub fn empty() -> BuildMode {
        BuildMode {
            choices: Vec::new(),
            names: Vec::new(),
            chosen: None,
            asked: None,
            pinned: None,
        }
    }

    /// The role name of `kind`, or its number when the definitions do not name it.
    pub fn name_of(&self, kind: Kind) -> String {
        self.names.iter().find(|(k, _)| *k == kind).map_or_else(
            || format!("structure {}", kind.0),
            |(_, n)| n.replace('_', " "),
        )
    }

    /// Every structure on offer, by kind.
    pub fn choices(&self) -> &[Choice] {
        &self.choices
    }

    /// The structure chosen, if one is.
    pub fn chosen(&self) -> Option<&Choice> {
        self.chosen.and_then(|i| self.choices.get(i))
    }

    /// Whether a structure is chosen: clicks place it and orders are off.
    pub fn is_active(&self) -> bool {
        self.chosen.is_some()
    }

    /// The next structure; after the last one none; from none the first.
    pub fn cycle(&mut self) {
        self.chosen = match self.chosen {
            None if !self.choices.is_empty() => Some(0),
            Some(i) if i + 1 < self.choices.len() => Some(i + 1),
            _ => None,
        };
    }

    /// Chooses the structure with role name `id`; false if the faction does not build it.
    pub fn choose(&mut self, id: &str) -> bool {
        match self.choices.iter().position(|c| c.id == id) {
            Some(i) => {
                self.chosen = Some(i);
                true
            }
            None => false,
        }
    }

    /// Puts the structure away.
    pub fn cancel(&mut self) {
        self.chosen = None;
    }

    /// From now on the ghost sits on `cell` whatever the cursor does.
    pub fn pin(&mut self, cell: Cell) {
        self.pinned = Some(cell);
    }

    /// The anchor cell of the ghost: the pinned one, or the one that puts the middle of the
    /// chosen footprint on `under_cursor` (not below the first row and column of the map).
    pub fn target(&self, under_cursor: Option<Cell>) -> Option<Cell> {
        if let Some(pinned) = self.pinned {
            return Some(pinned);
        }
        let cell = under_cursor?;
        let (w, h) = self.chosen().map_or((1, 1), |c| c.size);
        Some(Cell::new(
            cell.x.saturating_sub(u32::from(w) / 2),
            cell.y.saturating_sub(u32::from(h) / 2),
        ))
    }

    /// The question for the sim thread with the cursor over `under_cursor`: none when no
    /// structure is chosen or the cursor is over no cell.
    pub fn query(&self, player: PlayerId, under_cursor: Option<Cell>) -> Option<GhostQuery> {
        let cell = self.target(under_cursor)?;
        let kind = self.chosen()?.kind;
        Some(GhostQuery { kind, cell, player })
    }

    /// Tells the sim thread what to answer, when that changed since the last time. Returns
    /// whether it sent something.
    pub fn ask(&mut self, sim: &SimHandle, query: Option<GhostQuery>) -> bool {
        if query == self.asked {
            return false;
        }
        self.asked = query;
        sim.set_ghost(query);
        true
    }

    /// The command a left click sends: `BuildStructure` for the ghost's cell. `None` with no
    /// structure chosen or no cell under the cursor.
    pub fn click(&self, under_cursor: Option<Cell>) -> Option<Command> {
        let kind = self.chosen()?.kind;
        let cell = self.target(under_cursor)?;
        Some(Command::BuildStructure { kind, cell })
    }
}

#[cfg(test)]
mod tests;
