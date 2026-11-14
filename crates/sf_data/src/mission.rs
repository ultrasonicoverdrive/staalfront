//! The mission file (WP10.1a): the match file of WP7.6 with the things a mission needs on top of it.
//!
//! [`MissionDef`] is a **strict superset** of [`crate::match_def::MatchDef`]: the same field names with
//! the same meaning, and every new field is `#[serde(default)]`, so every match file that exists loads as
//! a mission with the defaults and means what it meant (`tests/mission_files.rs` shows it for every shipped
//! and every frozen file). What it adds:
//!
//! * per player `goals`: when the player is defeated (`defeat`, **any** goal holding is enough) and when it
//!   has won (`victory`, **all** goals must hold); left out it is today's rule ([`Goals::default`]);
//! * `zones`: named rectangles of cells; `variables`: named integers; `teams`: scripted groups (a
//!   composition and an order list, not alliances); `triggers`: event, condition and action lists;
//! * per object an optional `name`, which a trigger can refer to.
//!
//! **What this WP does and does not do.** It defines the types, reads them ([`MissionDef::from_ron`]) and
//! checks every reference and range ([`MissionDef::from_ron`] for what the file alone can say,
//! [`MissionDef::check`] against the unit definitions and the map). Nothing runs it: the simulation, the
//! bridge and the game still read `MatchDef`. The goal model in `sf_sim::outcome` is WP10.1b, the trigger
//! runtime WP10.2, the actions WP10.3. The vocabularies of [`Event`], [`Condition`], [`Action`] and
//! [`TeamOrder`] are **provisional**: they are the lists PLAN names for those work packages, fixed now so
//! the editor (WP10.4) has a target, and WP10.2 and WP10.3 may change them. See `docs/rules/mission.md`.
//!
//! The format has no floats: cells, ticks and counts are integers. Every list keeps the order of the
//! file (players give slots, triggers are evaluated in file order), except the goal lists, whose order
//! means nothing and which are sorted and de-duplicated on reading ([`Goals::canonical`]).

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;

use crate::match_def::{Controller, MAX_CREDITS, MAX_PLAYERS, MatchDef, MatchObject, MatchPlayer};
use crate::units::{BuildingFunction, Category, Faction, UnitDefs};

/// The longest a name (object, zone, variable, team, trigger) can be.
pub const MAX_NAME: usize = 32;
/// The most zones a mission can have.
pub const MAX_ZONES: usize = 64;
/// The most variables a mission can have.
pub const MAX_VARIABLES: usize = 64;
/// The most teams a mission can have.
pub const MAX_TEAMS: usize = 64;
/// The most triggers a mission can have.
pub const MAX_TRIGGERS: usize = 256;
/// The most member lines a team can have.
pub const MAX_TEAM_MEMBER_LINES: usize = 32;
/// The most of one role in a team member line.
pub const MAX_TEAM_COUNT: u32 = 64;
/// The most orders a team can have.
pub const MAX_TEAM_ORDERS: usize = 32;
/// The most events, conditions or actions a trigger can have, each.
pub const MAX_TRIGGER_PARTS: usize = 16;
/// The longest a tick count in the file can be (a time, a guard duration): 1,000,000 ticks.
pub const MAX_TICKS: u32 = 1_000_000;
/// A variable, a value written to one and a change to one lie between minus and plus this.
pub const MAX_VALUE: i32 = 1_000_000;
/// The longest a message can be, in characters.
pub const MAX_MESSAGE: usize = 200;

/// Why a mission file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionError(pub String);

impl fmt::Display for MissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mission file: {}", self.0)
    }
}

impl std::error::Error for MissionError {}

fn err<T>(message: impl Into<String>) -> Result<T, MissionError> {
    Err(MissionError(message.into()))
}

/// One object a player starts with: as [`MatchObject`], plus an optional name.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionObject {
    /// The id of a role in `unit_defs.ron`.
    pub role: String,
    /// The cell `(x, y)`: a structure is anchored on it, a unit stands in the middle of it.
    pub cell: (u32, u32),
    /// What triggers call it. Unique in the whole file.
    #[serde(default)]
    pub name: Option<String>,
}

/// One thing a goal asks about a player. The four here are the ones the state scan of
/// `sf_sim::outcome` can answer; more may be added.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Goal {
    /// The player owns no standing structure with this building function (the default defeat rule is
    /// `NoStanding(function: Anchor)`: the construction centre).
    NoStanding {
        /// The function.
        function: BuildingFunction,
    },
    /// The player owns no standing structure of any of these roles.
    NoStandingRoles {
        /// The role ids (sorted and without doubles after reading).
        roles: Vec<String>,
    },
    /// The player owns no standing structure and no armed unit (the rule of WP7.6).
    NothingLeft,
    /// Every other player is defeated. A victory goal only.
    LastInMatch,
}

/// A player's goals. Left out of the file this is [`Goals::default`], today's rule.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Goals {
    /// The player is defeated when **any** of these holds. Empty: it cannot be defeated by the state of
    /// the world (a trigger can still make it lose).
    #[serde(default = "default_defeat")]
    pub defeat: Vec<Goal>,
    /// The player has won when **all** of these hold. Empty: it cannot win by the state of the world
    /// (never vacuously true; a trigger can still make it win).
    #[serde(default = "default_victory")]
    pub victory: Vec<Goal>,
}

fn default_defeat() -> Vec<Goal> {
    vec![Goal::NoStanding {
        function: BuildingFunction::Anchor,
    }]
}

fn default_victory() -> Vec<Goal> {
    vec![Goal::LastInMatch]
}

impl Default for Goals {
    fn default() -> Goals {
        Goals {
            defeat: default_defeat(),
            victory: default_victory(),
        }
    }
}

impl Goals {
    /// Whether these are the default goals (the only case the simulation will not hash, WP10.1b).
    pub fn is_default(&self) -> bool {
        *self == Goals::default()
    }

    /// The goals with both lists sorted and without doubles: the order of a list means nothing, so this
    /// is the one form that is hashed and compared.
    pub fn canonical(&self) -> Goals {
        fn canon(list: &[Goal]) -> Vec<Goal> {
            let mut out: Vec<Goal> = list
                .iter()
                .map(|g| match g {
                    Goal::NoStandingRoles { roles } => {
                        let mut r = roles.clone();
                        r.sort();
                        r.dedup();
                        Goal::NoStandingRoles { roles: r }
                    }
                    other => other.clone(),
                })
                .collect();
            out.sort();
            out.dedup();
            out
        }
        Goals {
            defeat: canon(&self.defeat),
            victory: canon(&self.victory),
        }
    }

    /// **Any** defeat goal holds: the player is defeated. `holds` answers one goal for this player.
    pub fn is_defeated(&self, holds: impl FnMut(&Goal) -> bool) -> bool {
        self.defeat.iter().any(holds)
    }

    /// **All** victory goals hold, and there is at least one: the player has won.
    pub fn has_won(&self, holds: impl FnMut(&Goal) -> bool) -> bool {
        !self.victory.is_empty() && self.victory.iter().all(holds)
    }
}

/// One player: as [`MatchPlayer`], plus goals.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionPlayer {
    /// The player number.
    pub id: u8,
    /// Which faction the player is.
    pub faction: Faction,
    /// The credits it starts with, 0 to [`MAX_CREDITS`].
    pub credits: u64,
    /// What it starts with, in the order the objects enter the world.
    pub objects: Vec<MissionObject>,
    /// Who plays it.
    #[serde(default)]
    pub controller: Controller,
    /// When it is defeated and when it has won.
    #[serde(default)]
    pub goals: Goals,
}

/// A named rectangle of cells, both corners included.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    /// Unique name.
    pub name: String,
    /// The smallest x and y.
    pub min: (u32, u32),
    /// The largest x and y.
    pub max: (u32, u32),
}

/// A named integer that triggers read and write.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    /// Unique name.
    pub name: String,
    /// The value at the start, between minus and plus [`MAX_VALUE`].
    #[serde(default)]
    pub start: i32,
}

/// One step of a team's order list (provisional).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum TeamOrder {
    /// Move to the middle of a zone.
    MoveTo {
        /// A zone.
        zone: String,
    },
    /// Move to a zone and fight what is met on the way.
    AttackMove {
        /// A zone.
        zone: String,
    },
    /// Stay and fight what comes, for this many ticks.
    Guard {
        /// 1 to [`MAX_TICKS`].
        ticks: u32,
    },
    /// Go back to an earlier step (its index in this list, counted from 0, at most this step's own).
    Loop {
        /// The step.
        to: u32,
    },
    /// Write a variable.
    SetVariable {
        /// A variable.
        variable: String,
        /// The value.
        value: i32,
    },
}

/// A scripted group: a composition and an order list. Not an alliance: nothing here changes who is
/// hostile to whom.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    /// Unique name.
    pub name: String,
    /// The player the team belongs to.
    pub owner: u8,
    /// `(role, count)` lines, 1 to [`MAX_TEAM_COUNT`] of each.
    pub members: Vec<(String, u32)>,
    /// What the team does, step by step.
    #[serde(default)]
    pub orders: Vec<TeamOrder>,
}

/// What a trigger waits for (provisional; the runtime is WP10.2).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Event {
    /// Always true when evaluated.
    Always,
    /// This many ticks after the start.
    Time {
        /// 1 to [`MAX_TICKS`].
        ticks: u32,
    },
    /// A unit of the trigger's owner enters a zone.
    EnterZone {
        /// A zone.
        zone: String,
    },
    /// A named start object is destroyed.
    Destroyed {
        /// An object name.
        object: String,
    },
    /// Everything of a player is destroyed.
    AllDestroyed {
        /// A player.
        player: u8,
    },
    /// A player's money rises above a threshold.
    MoneyAbove {
        /// A player.
        player: u8,
        /// 0 to [`MAX_CREDITS`].
        credits: u64,
    },
    /// A player has built a structure or made a unit of a role.
    Built {
        /// A player.
        player: u8,
        /// A role id.
        role: String,
    },
    /// A player's power is short.
    PowerShortage {
        /// A player.
        player: u8,
    },
}

/// What must also hold when an event fires a trigger (provisional).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Condition {
    /// A variable has exactly this value.
    VariableIs {
        /// A variable.
        variable: String,
        /// The value.
        value: i32,
    },
    /// A variable is at least this.
    VariableAtLeast {
        /// A variable.
        variable: String,
        /// The value.
        value: i32,
    },
    /// A named start object still stands.
    ObjectStanding {
        /// An object name.
        object: String,
    },
    /// A team has been created and has members left.
    TeamAlive {
        /// A team.
        team: String,
    },
    /// A player has at least this many credits.
    MoneyAtLeast {
        /// A player.
        player: u8,
        /// 0 to [`MAX_CREDITS`].
        credits: u64,
    },
}

/// What a trigger does (provisional; the runtime is WP10.3).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Action {
    /// The player wins.
    Win {
        /// A player.
        player: u8,
    },
    /// The player loses.
    Lose {
        /// A player.
        player: u8,
    },
    /// Show a text.
    Message {
        /// 1 to [`MAX_MESSAGE`] characters.
        text: String,
    },
    /// Create a team in a zone.
    SpawnTeam {
        /// A team.
        team: String,
        /// A zone.
        zone: String,
    },
    /// Disband a team.
    DisbandTeam {
        /// A team.
        team: String,
    },
    /// Write a variable.
    SetVariable {
        /// A variable.
        variable: String,
        /// The value.
        value: i32,
    },
    /// Add to a variable (the change may be negative).
    AddVariable {
        /// A variable.
        variable: String,
        /// The change.
        delta: i32,
    },
    /// Lift the fog over a zone for the trigger's owner.
    RevealZone {
        /// A zone.
        zone: String,
    },
    /// Move the camera to a zone.
    CameraTo {
        /// A zone.
        zone: String,
    },
    /// Destroy a named start object.
    Destroy {
        /// An object name.
        object: String,
    },
    /// Fire another trigger.
    ForceTrigger {
        /// A trigger name.
        trigger: String,
    },
}

/// How often a trigger can fire.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Lifetime {
    /// Once; then it is gone.
    Once,
    /// Every time its event comes.
    Persistent,
}

/// event, condition, action. It fires when **any** listed event happens and **all** conditions hold.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    /// Unique name.
    pub name: String,
    /// The player the trigger belongs to.
    pub owner: u8,
    /// Once or persistent.
    pub lifetime: Lifetime,
    /// At least one.
    pub events: Vec<Event>,
    /// Possibly none.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// At least one.
    pub actions: Vec<Action>,
}

/// The file as written.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionDef {
    /// A short name for the console and the window.
    pub name: String,
    /// The map: the name of a directory under `assets/maps`.
    pub map: String,
    /// The world seed.
    pub seed: u64,
    /// The players, in the order their objects are made.
    pub players: Vec<MissionPlayer>,
    /// Named rectangles.
    #[serde(default)]
    pub zones: Vec<Zone>,
    /// Named integers.
    #[serde(default)]
    pub variables: Vec<Variable>,
    /// Scripted groups (format only until WP10.2).
    #[serde(default)]
    pub teams: Vec<Team>,
    /// Event, condition, action (format only until WP10.2), in the order they are evaluated.
    #[serde(default)]
    pub triggers: Vec<Trigger>,
}

/// The names a reference can point at.
struct Names<'a> {
    players: BTreeSet<u8>,
    zones: BTreeSet<&'a str>,
    variables: BTreeSet<&'a str>,
    teams: BTreeSet<&'a str>,
    objects: BTreeSet<&'a str>,
    triggers: BTreeSet<&'a str>,
}

impl Names<'_> {
    fn player(&self, at: &str, field: &str, id: u8) -> Result<(), MissionError> {
        if self.players.contains(&id) {
            Ok(())
        } else {
            err(format!("{at}: {field} {id} is not a listed player"))
        }
    }
    fn zone(&self, at: &str, name: &str) -> Result<(), MissionError> {
        reference(at, "zone", name, &self.zones, "zones")
    }
    fn variable(&self, at: &str, name: &str) -> Result<(), MissionError> {
        reference(at, "variable", name, &self.variables, "variables")
    }
    fn team(&self, at: &str, name: &str) -> Result<(), MissionError> {
        reference(at, "team", name, &self.teams, "teams")
    }
    fn object(&self, at: &str, name: &str) -> Result<(), MissionError> {
        reference(at, "object", name, &self.objects, "named objects")
    }
    fn trigger(&self, at: &str, name: &str) -> Result<(), MissionError> {
        reference(at, "trigger", name, &self.triggers, "triggers")
    }
}

fn reference(
    at: &str,
    field: &str,
    name: &str,
    set: &BTreeSet<&str>,
    what: &str,
) -> Result<(), MissionError> {
    if set.contains(name) {
        Ok(())
    } else {
        err(format!("{at}: {field} \"{name}\" is not among the {what}"))
    }
}

/// A name of lower-case letters, digits and '_', 1 to [`MAX_NAME`] long.
fn check_name(at: &str, field: &str, name: &str) -> Result<(), MissionError> {
    if name.is_empty()
        || name.len() > MAX_NAME
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return err(format!(
            "{at}: {field} \"{name}\" must be 1 to {MAX_NAME} lower-case letters, digits or '_'"
        ));
    }
    Ok(())
}

fn unique<'a>(
    set: &mut BTreeSet<&'a str>,
    at: &str,
    field: &str,
    name: &'a str,
) -> Result<(), MissionError> {
    check_name(at, field, name)?;
    if !set.insert(name) {
        return err(format!("{at}: {field} \"{name}\" is used twice"));
    }
    Ok(())
}

fn in_range(at: &str, field: &str, value: i64, lo: i64, hi: i64) -> Result<(), MissionError> {
    if (lo..=hi).contains(&value) {
        Ok(())
    } else {
        err(format!("{at}: {field} {value} is outside {lo} to {hi}"))
    }
}

fn credits_in_range(at: &str, credits: u64) -> Result<(), MissionError> {
    if credits <= MAX_CREDITS {
        Ok(())
    } else {
        err(format!(
            "{at}: credits {credits} is above the limit of {MAX_CREDITS}"
        ))
    }
}

fn value_in_range(at: &str, field: &str, v: i32) -> Result<(), MissionError> {
    in_range(at, field, v.into(), -i64::from(MAX_VALUE), MAX_VALUE.into())
}

impl MissionDef {
    /// Reads a mission file and checks everything the file can say by itself: the shape, the players, every
    /// name, every reference (zone, object, team, variable, trigger, player) and every range. The goal
    /// lists are put in canonical form. The error names the field. What needs the unit definitions or the
    /// map (roles, cells on the map) is [`MissionDef::check`].
    pub fn from_ron(text: &str) -> Result<MissionDef, MissionError> {
        let mut def: MissionDef = ron::from_str(text).map_err(|e| MissionError(e.to_string()))?;
        def.validate()?;
        for p in &mut def.players {
            p.goals = p.goals.canonical();
        }
        Ok(def)
    }

    /// This mission as the match file it extends: the same name, map, seed, players and objects, without
    /// what a match file cannot say.
    pub fn as_match_def(&self) -> MatchDef {
        MatchDef {
            name: self.name.clone(),
            map: self.map.clone(),
            seed: self.seed,
            players: self
                .players
                .iter()
                .map(|p| MatchPlayer {
                    id: p.id,
                    faction: p.faction,
                    credits: p.credits,
                    objects: p
                        .objects
                        .iter()
                        .map(|o| MatchObject {
                            role: o.role.clone(),
                            cell: o.cell,
                        })
                        .collect(),
                    controller: p.controller.clone(),
                })
                .collect(),
        }
    }

    fn validate(&self) -> Result<(), MissionError> {
        // The part a match file has too.
        if self.name.trim().is_empty() {
            return err("name must not be empty");
        }
        crate::map::check_file_name("map", &self.map).map_err(|e| MissionError(e.to_string()))?;
        if self.map.contains('/') {
            return err(format!(
                "map \"{}\" must be the name of one map directory, without '/'",
                self.map
            ));
        }
        if !(2..=MAX_PLAYERS).contains(&self.players.len()) {
            return err(format!(
                "{} players listed, a mission has 2 to {MAX_PLAYERS}",
                self.players.len()
            ));
        }
        let mut names = Names {
            players: BTreeSet::new(),
            zones: BTreeSet::new(),
            variables: BTreeSet::new(),
            teams: BTreeSet::new(),
            objects: BTreeSet::new(),
            triggers: BTreeSet::new(),
        };
        for p in &self.players {
            let at = format!("player {}", p.id);
            if !names.players.insert(p.id) {
                return err(format!("{at}: player {} is listed twice", p.id));
            }
            if p.faction == Faction::Neutral {
                return err(format!(
                    "{at}: the faction cannot be Neutral (nobody plays it)"
                ));
            }
            if p.credits > MAX_CREDITS {
                return err(format!(
                    "{at}: {} credits is above the limit of {MAX_CREDITS}",
                    p.credits
                ));
            }
            if p.objects.is_empty() {
                return err(format!("{at}: starts with nothing"));
            }
            if let Controller::Ai { profile } = &p.controller
                && (profile.is_empty()
                    || !profile
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
            {
                return err(format!(
                    "{at}: ai profile \"{profile}\" must be a name of lower-case letters, digits and '_' \
                     (the file assets/ai/<name>.ron), like \"normal\""
                ));
            }
            for (i, o) in p.objects.iter().enumerate() {
                if let Some(n) = &o.name {
                    unique(
                        &mut names.objects,
                        &format!("{at} object {i} ({})", o.role),
                        "name",
                        n,
                    )?;
                }
            }
            check_goals(&p.goals, &at)?;
        }
        // Names of the new lists, before anything refers to them.
        let sizes = [
            ("zones", self.zones.len(), MAX_ZONES),
            ("variables", self.variables.len(), MAX_VARIABLES),
            ("teams", self.teams.len(), MAX_TEAMS),
            ("triggers", self.triggers.len(), MAX_TRIGGERS),
        ];
        for (field, n, max) in sizes {
            if n > max {
                return err(format!("{field} has {n} entries, at most {max}"));
            }
        }
        for (i, z) in self.zones.iter().enumerate() {
            let at = format!("zones[{i}]");
            unique(&mut names.zones, &at, "name", &z.name)?;
            if z.min.0 > z.max.0 || z.min.1 > z.max.1 {
                return err(format!(
                    "{at} \"{}\": min {:?} must not be above max {:?} in x or y",
                    z.name, z.min, z.max
                ));
            }
        }
        for (i, v) in self.variables.iter().enumerate() {
            let at = format!("variables[{i}]");
            unique(&mut names.variables, &at, "name", &v.name)?;
            value_in_range(&at, "start", v.start)?;
        }
        for (i, t) in self.teams.iter().enumerate() {
            unique(&mut names.teams, &format!("teams[{i}]"), "name", &t.name)?;
        }
        for (i, t) in self.triggers.iter().enumerate() {
            unique(
                &mut names.triggers,
                &format!("triggers[{i}]"),
                "name",
                &t.name,
            )?;
        }
        // The references.
        for (i, t) in self.teams.iter().enumerate() {
            check_team(&format!("teams[{i}] \"{}\"", t.name), t, &names)?;
        }
        for (i, t) in self.triggers.iter().enumerate() {
            check_trigger(&format!("triggers[{i}] \"{}\"", t.name), t, &names)?;
        }
        Ok(())
    }

    /// Checks the mission against the unit definitions and the size of its map (in cells): what
    /// [`MatchDef::check`] checks of the objects (the role exists, is a structure, infantry or a
    /// vehicle, is open to the faction, lies on the map, structures do not overlap), the roles of
    /// goals, team members and `Built` events, the zones lie on the map, and every player starts with
    /// what its own defeat goals ask for: a player whose defeat goal is `NoStanding` of a function or of
    /// roles must start with a structure of it (it would be defeated before it began); with the default
    /// goals that is the construction centre of the match file.
    pub fn check(&self, defs: &UnitDefs, map_cells: (u32, u32)) -> Result<(), MissionError> {
        let mut taken = BTreeSet::new();
        for p in &self.players {
            let mut started_functions = BTreeSet::new();
            let mut started_roles = BTreeSet::new();
            for (i, o) in p.objects.iter().enumerate() {
                let at = format!("player {} object {i} ({})", p.id, o.role);
                let Some(def) = defs.unit(&o.role) else {
                    return err(format!("{at}: no such role in unit_defs.ron"));
                };
                if !def.factions.contains(&p.faction) {
                    return err(format!(
                        "{at}: the role is not open to the faction {:?}",
                        p.faction
                    ));
                }
                let (w, h) = match def.category {
                    Category::Structure => defs
                        .sim_unit(def.kind)
                        .and_then(|u| u.footprint)
                        .ok_or_else(|| {
                            MissionError(format!("{at}: a structure without a footprint"))
                        })?,
                    Category::Infantry | Category::Vehicle => (1, 1),
                    Category::Vessel | Category::Object => {
                        return err(format!(
                            "{at}: only structures, infantry and vehicles can start a mission"
                        ));
                    }
                };
                if def.category == Category::Structure {
                    started_roles.insert(o.role.as_str());
                    if let Some(b) = &def.building {
                        started_functions.insert(b.function);
                    }
                }
                let (x1, y1) = (o.cell.0 + u32::from(w), o.cell.1 + u32::from(h));
                if x1 > map_cells.0 || y1 > map_cells.1 {
                    return err(format!(
                        "{at}: cell {:?} with its footprint lies outside the {}x{} map",
                        o.cell, map_cells.0, map_cells.1
                    ));
                }
                if def.category == Category::Structure {
                    for y in o.cell.1..y1 {
                        for x in o.cell.0..x1 {
                            if !taken.insert((x, y)) {
                                return err(format!(
                                    "{at}: overlaps another structure on cell ({x}, {y})"
                                ));
                            }
                        }
                    }
                }
            }
            for g in p.goals.defeat.iter().chain(&p.goals.victory) {
                if let Goal::NoStandingRoles { roles } = g {
                    for r in roles {
                        match defs.unit(r) {
                            Some(d) if d.category == Category::Structure => {}
                            Some(_) => {
                                return err(format!(
                                    "player {} goals: role \"{r}\" is not a structure",
                                    p.id
                                ));
                            }
                            None => {
                                return err(format!(
                                    "player {} goals: role \"{r}\" is not in unit_defs.ron",
                                    p.id
                                ));
                            }
                        }
                    }
                }
            }
            for g in &p.goals.defeat {
                match g {
                    Goal::NoStanding { function } if !started_functions.contains(function) => {
                        return err(format!(
                            "player {} starts without a structure of the function {function:?}, \
                             which its defeat goal NoStanding asks for: it would be defeated before it began \
                             (the default goals ask for a construction center)",
                            p.id
                        ));
                    }
                    Goal::NoStandingRoles { roles }
                        if !roles.iter().any(|r| started_roles.contains(r.as_str())) =>
                    {
                        return err(format!(
                            "player {} starts without a structure of any of the roles {roles:?}, \
                             which its defeat goal NoStandingRoles asks for: it would be defeated before it began",
                            p.id
                        ));
                    }
                    _ => {}
                }
            }
        }
        for (i, z) in self.zones.iter().enumerate() {
            if z.max.0 >= map_cells.0 || z.max.1 >= map_cells.1 {
                return err(format!(
                    "zones[{i}] \"{}\": max {:?} lies outside the {}x{} map",
                    z.name, z.max, map_cells.0, map_cells.1
                ));
            }
        }
        for (i, t) in self.teams.iter().enumerate() {
            let faction = self
                .players
                .iter()
                .find(|p| p.id == t.owner)
                .map_or(Faction::Neutral, |p| p.faction);
            for (m, (role, _)) in t.members.iter().enumerate() {
                let at = format!("teams[{i}] \"{}\" members[{m}]", t.name);
                let Some(d) = defs.unit(role) else {
                    return err(format!("{at}: role \"{role}\" is not in unit_defs.ron"));
                };
                if !matches!(d.category, Category::Infantry | Category::Vehicle) {
                    return err(format!(
                        "{at}: role \"{role}\" is not infantry or a vehicle"
                    ));
                }
                if !d.factions.contains(&faction) {
                    return err(format!(
                        "{at}: role \"{role}\" is not open to the faction {faction:?}"
                    ));
                }
            }
        }
        for (i, t) in self.triggers.iter().enumerate() {
            for (e, ev) in t.events.iter().enumerate() {
                if let Event::Built { role, .. } = ev
                    && defs.unit(role).is_none()
                {
                    return err(format!(
                        "triggers[{i}] \"{}\" events[{e}]: role \"{role}\" is not in unit_defs.ron",
                        t.name
                    ));
                }
            }
        }
        Ok(())
    }
}

fn check_goals(goals: &Goals, at: &str) -> Result<(), MissionError> {
    if goals.defeat.contains(&Goal::LastInMatch) {
        return err(format!(
            "{at}: goals.defeat: LastInMatch is a victory goal (it would defeat the winner)"
        ));
    }
    for (side, list) in [("defeat", &goals.defeat), ("victory", &goals.victory)] {
        for g in list {
            if let Goal::NoStandingRoles { roles } = g {
                if roles.is_empty() {
                    return err(format!("{at}: goals.{side}: NoStandingRoles needs roles"));
                }
                for r in roles {
                    check_name(&format!("{at}: goals.{side}"), "role", r)?;
                }
            }
        }
    }
    Ok(())
}

fn check_team(at: &str, t: &Team, names: &Names) -> Result<(), MissionError> {
    names.player(at, "owner", t.owner)?;
    if t.members.is_empty() || t.members.len() > MAX_TEAM_MEMBER_LINES {
        return err(format!(
            "{at}: members has {} lines, a team has 1 to {MAX_TEAM_MEMBER_LINES}",
            t.members.len()
        ));
    }
    for (m, (role, count)) in t.members.iter().enumerate() {
        let at = format!("{at} members[{m}]");
        check_name(&at, "role", role)?;
        in_range(&at, "count", (*count).into(), 1, MAX_TEAM_COUNT.into())?;
    }
    if t.orders.len() > MAX_TEAM_ORDERS {
        return err(format!(
            "{at}: orders has {} steps, at most {MAX_TEAM_ORDERS}",
            t.orders.len()
        ));
    }
    for (s, o) in t.orders.iter().enumerate() {
        let at = format!("{at} orders[{s}]");
        match o {
            TeamOrder::MoveTo { zone } | TeamOrder::AttackMove { zone } => names.zone(&at, zone)?,
            TeamOrder::Guard { ticks } => {
                in_range(&at, "ticks", (*ticks).into(), 1, MAX_TICKS.into())?;
            }
            TeamOrder::Loop { to } => in_range(&at, "to", (*to).into(), 0, s as i64)?,
            TeamOrder::SetVariable { variable, value } => {
                names.variable(&at, variable)?;
                value_in_range(&at, "value", *value)?;
            }
        }
    }
    Ok(())
}

fn check_trigger(at: &str, t: &Trigger, names: &Names) -> Result<(), MissionError> {
    names.player(at, "owner", t.owner)?;
    for (field, n, min) in [
        ("events", t.events.len(), 1),
        ("conditions", t.conditions.len(), 0),
        ("actions", t.actions.len(), 1),
    ] {
        if n < min || n > MAX_TRIGGER_PARTS {
            return err(format!(
                "{at}: {field} has {n} entries, a trigger has {min} to {MAX_TRIGGER_PARTS}"
            ));
        }
    }
    for (i, e) in t.events.iter().enumerate() {
        let at = format!("{at} events[{i}]");
        match e {
            Event::Always => {}
            Event::Time { ticks } => in_range(&at, "ticks", (*ticks).into(), 1, MAX_TICKS.into())?,
            Event::EnterZone { zone } => names.zone(&at, zone)?,
            Event::Destroyed { object } => names.object(&at, object)?,
            Event::AllDestroyed { player } | Event::PowerShortage { player } => {
                names.player(&at, "player", *player)?;
            }
            Event::MoneyAbove { player, credits } => {
                names.player(&at, "player", *player)?;
                credits_in_range(&at, *credits)?;
            }
            Event::Built { player, role } => {
                names.player(&at, "player", *player)?;
                check_name(&at, "role", role)?;
            }
        }
    }
    for (i, c) in t.conditions.iter().enumerate() {
        let at = format!("{at} conditions[{i}]");
        match c {
            Condition::VariableIs { variable, value }
            | Condition::VariableAtLeast { variable, value } => {
                names.variable(&at, variable)?;
                value_in_range(&at, "value", *value)?;
            }
            Condition::ObjectStanding { object } => names.object(&at, object)?,
            Condition::TeamAlive { team } => names.team(&at, team)?,
            Condition::MoneyAtLeast { player, credits } => {
                names.player(&at, "player", *player)?;
                credits_in_range(&at, *credits)?;
            }
        }
    }
    for (i, a) in t.actions.iter().enumerate() {
        let at = format!("{at} actions[{i}]");
        match a {
            Action::Win { player } | Action::Lose { player } => {
                names.player(&at, "player", *player)?;
            }
            Action::Message { text } => in_range(
                &at,
                "text length",
                text.chars().count() as i64,
                1,
                MAX_MESSAGE as i64,
            )?,
            Action::SpawnTeam { team, zone } => {
                names.team(&at, team)?;
                names.zone(&at, zone)?;
            }
            Action::DisbandTeam { team } => names.team(&at, team)?,
            Action::SetVariable { variable, value } => {
                names.variable(&at, variable)?;
                value_in_range(&at, "value", *value)?;
            }
            Action::AddVariable { variable, delta } => {
                names.variable(&at, variable)?;
                value_in_range(&at, "delta", *delta)?;
            }
            Action::RevealZone { zone } | Action::CameraTo { zone } => names.zone(&at, zone)?,
            Action::Destroy { object } => names.object(&at, object)?,
            Action::ForceTrigger { trigger } => names.trigger(&at, trigger)?,
        }
    }
    Ok(())
}
