//! The sidebar's content (WP11.1, `docs/rules/sidebar.md`): which items a build tab shows, what
//! each item says about itself, and what a click on one does. Presentation only: the prices, the
//! build times and the prerequisites are the unit definitions', the queues, the progress and the
//! credits the simulation's (copied into the snapshot); nothing here decides a rule. A click
//! becomes an [`Action`], which the runner turns into the commands and the build mode the debug
//! keys of WP6.5 already use; the simulation checks every one again at its tick.
//!
//! The geometry is [`crate::sidebar_layout`], the drawing [`crate::sidebar_view`]; this file is
//! pure and needs no window.

use sf_data::sidebar_look::SidebarLook;
use sf_data::units::{Category, Faction, UnitDefs};
use sf_sim::{BuildingFunction, Command, EntityId, FactoryStatus, FactoryView, Kind, PlayerId};

use crate::sim_snapshot::SimSnapshot;

/// The four build tabs, in the order they are shown.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tab {
    Structures,
    Infantry,
    Vehicles,
    Vessels,
}

impl Tab {
    /// Every tab, in the order they are shown.
    pub const ALL: [Tab; 4] = [Tab::Structures, Tab::Infantry, Tab::Vehicles, Tab::Vessels];

    /// The place of the tab in [`Tab::ALL`].
    pub const fn index(self) -> usize {
        match self {
            Tab::Structures => 0,
            Tab::Infantry => 1,
            Tab::Vehicles => 2,
            Tab::Vessels => 3,
        }
    }

    /// The tab a role of `category` is built from; none for an object (nothing builds it).
    pub const fn of_category(category: Category) -> Option<Tab> {
        match category {
            Category::Structure => Some(Tab::Structures),
            Category::Infantry => Some(Tab::Infantry),
            Category::Vehicle => Some(Tab::Vehicles),
            Category::Vessel => Some(Tab::Vessels),
            Category::Object => None,
        }
    }

    /// Whatever is in `per_tab` for this tab.
    pub fn pick<T>(self, per_tab: &(T, T, T, T)) -> &T {
        match self {
            Tab::Structures => &per_tab.0,
            Tab::Infantry => &per_tab.1,
            Tab::Vehicles => &per_tab.2,
            Tab::Vessels => &per_tab.3,
        }
    }
}

/// The picture of an item: a coloured tile with a short name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// The colour of the tile.
    pub color: (u8, u8, u8),
    /// The short name on it.
    pub label: String,
}

/// The function of the factory that builds a unit of `category`: the simulation's own type (the
/// data crate has one of its own with the same numbers, `sf_data::units::produced_by`; a test
/// compares the two). `None` for a structure, which no factory builds.
pub const fn factory_for(category: Category) -> Option<BuildingFunction> {
    match category {
        Category::Infantry => Some(BuildingFunction::InfantryFactory),
        Category::Vehicle => Some(BuildingFunction::VehicleFactory),
        Category::Vessel => Some(BuildingFunction::VesselFactory),
        Category::Structure | Category::Object => None,
    }
}

/// The factory a unit tab is built from; `None` for the structures tab.
pub const fn factory_of_tab(tab: Tab) -> Option<BuildingFunction> {
    match tab {
        Tab::Structures => None,
        Tab::Infantry => Some(BuildingFunction::InfantryFactory),
        Tab::Vehicles => Some(BuildingFunction::VehicleFactory),
        Tab::Vessels => Some(BuildingFunction::VesselFactory),
    }
}

/// One role the player's faction can build, from the definitions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The permanent kind number.
    pub kind: Kind,
    /// The role name (`power_plant`).
    pub id: String,
    /// The name for people (`power plant`).
    pub name: String,
    /// The tab it is built from.
    pub tab: Tab,
    /// Price in credits.
    pub cost: u32,
    /// Ticks to build.
    pub ticks: u32,
    /// The structures that must stand, besides the factory of a unit.
    pub requires: Vec<Kind>,
    /// For a unit: the function of the factory that builds it.
    pub factory: Option<BuildingFunction>,
    /// The picture.
    pub picture: Picture,
    /// Where it stands in its tab: ascending. The roles with a `build_picture` come first, in the
    /// order they name; the others after them, by kind.
    pub order: u32,
}

/// Everything the player's faction can build, in the order the tabs show it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    entries: Vec<Entry>,
    /// The kinds of all structures of the definitions, ascending: what counts as "standing".
    structures: Vec<Kind>,
    /// The kinds whose building function is the anchor (the construction centre), ascending: one of
    /// them must stand before the simulation places any structure.
    anchors: Vec<Kind>,
}

impl Catalog {
    /// The roles of `defs` that have a price and that `faction` may have, by tab, then by the
    /// order of their `build_picture` (the roles without one after those, by kind).
    pub fn build(defs: &UnitDefs, faction: Faction, look: &SidebarLook) -> Catalog {
        let mut entries = Vec::new();
        let mut structures = Vec::new();
        let mut anchors = Vec::new();
        for d in defs.units() {
            if d.category == Category::Structure {
                structures.push(Kind(d.kind));
                if d.building
                    .as_ref()
                    .is_some_and(|b| b.function == sf_data::units::BuildingFunction::Anchor)
                {
                    anchors.push(Kind(d.kind));
                }
            }
            let (Some(cost), Some(tab)) = (d.cost, Tab::of_category(d.category)) else {
                continue;
            };
            if !d.factions.contains(&faction) {
                continue;
            }
            let ids = match (&d.building, d.category) {
                (Some(b), Category::Structure) => &b.requires,
                _ => &d.requires,
            };
            let requires = ids
                .iter()
                .filter_map(|id| defs.kind_of(id).map(Kind))
                .collect();
            let (picture, order) = match &d.build_picture {
                Some(p) => (
                    Picture {
                        color: p.color,
                        label: p.label.clone(),
                    },
                    u32::from(p.order),
                ),
                None => (
                    Picture {
                        color: *tab.pick(&look.default_colors),
                        label: d
                            .id
                            .chars()
                            .take(usize::from(look.default_label_chars))
                            .collect::<String>()
                            .to_ascii_uppercase(),
                    },
                    1000 + u32::from(d.kind),
                ),
            };
            entries.push(Entry {
                kind: Kind(d.kind),
                id: d.id.clone(),
                name: d.id.replace('_', " "),
                tab,
                cost,
                ticks: d.build_seconds.to_ticks().unwrap_or(0),
                requires,
                factory: factory_for(d.category),
                picture,
                order,
            });
        }
        entries.sort_by_key(|e| (e.tab, e.order, e.kind));
        structures.sort();
        anchors.sort();
        Catalog {
            entries,
            structures,
            anchors,
        }
    }

    /// Every entry, by tab and order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry of `kind`.
    pub fn entry(&self, kind: Kind) -> Option<&Entry> {
        self.entries.iter().find(|e| e.kind == kind)
    }

    /// The name of `kind` (a role with or without a price), or its number.
    pub fn name_of(&self, kind: Kind) -> String {
        self.entry(kind)
            .map_or_else(|| format!("structure {}", kind.0), |e| e.name.clone())
    }

    /// Whether `kind` is a structure of the definitions.
    pub fn is_structure(&self, kind: Kind) -> bool {
        self.structures.binary_search(&kind).is_ok()
    }

    /// Whether `kind` is an anchor: the player builds round a standing one.
    pub fn is_anchor(&self, kind: Kind) -> bool {
        self.anchors.binary_search(&kind).is_ok()
    }
}

/// One factory of the player that stands, with its queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnFactory {
    /// The structure.
    pub id: EntityId,
    /// What it builds.
    pub function: BuildingFunction,
    /// Whether it is selected.
    pub selected: bool,
    /// Its queue, if it has one.
    pub view: Option<FactoryView>,
}

/// One structure of the player that is being built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Building {
    /// What it is.
    pub kind: Kind,
    /// Progress in thousandths.
    pub permille: u16,
    /// Whether it waits for credits.
    pub stalled: bool,
}

/// What the sidebar reads of the world: the snapshot's facts about the local player, taken once
/// per frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Situation {
    /// The kinds of the structures of the player that stand, ascending, each once.
    pub standing: Vec<Kind>,
    /// The factories of the player that stand, ascending by slot.
    pub factories: Vec<OwnFactory>,
    /// The structures of the player that are being built, ascending by slot.
    pub buildings: Vec<Building>,
    /// The credits of the player; `None` in a world without money rules.
    pub credits: Option<u64>,
    /// Whether an anchor of the player stands (a construction centre): without one the simulation
    /// refuses every placement, so the structures tab shows nothing.
    pub anchor: bool,
}

impl Situation {
    /// Reads what `player` has in `snap`; `selected` are the ids of the selected entities.
    pub fn read(
        snap: &SimSnapshot,
        player: PlayerId,
        catalog: &Catalog,
        selected: &[EntityId],
    ) -> Situation {
        let mut s = Situation {
            credits: snap.credits_of(player),
            ..Situation::default()
        };
        for v in snap.views() {
            if v.owner != player || !catalog.is_structure(v.kind) {
                continue;
            }
            match snap.construction_of(v.id) {
                Some(c) => s.buildings.push(Building {
                    kind: v.kind,
                    permille: c.permille(),
                    stalled: snap.stalled().contains(&v.id),
                }),
                // The simulation's `World::is_standing`: complete, and hit points left or a kind
                // that cannot be damaged (0 in the unit table). A wreck-to-be at 0 HP stands not.
                None if v.hp == 0 && !snap.stats_for(v).is_some_and(|t| t.hp == 0) => {}
                None => {
                    s.standing.push(v.kind);
                    s.anchor |= catalog.is_anchor(v.kind);
                    if let Some(function) = snap.menu().factory_function(v.kind) {
                        s.factories.push(OwnFactory {
                            id: v.id,
                            function,
                            selected: selected.contains(&v.id),
                            view: snap.factory_of(v.id).cloned(),
                        });
                    }
                }
            }
        }
        s.standing.sort();
        s.standing.dedup();
        s.factories.sort_by_key(|f| f.id.index());
        s
    }

    /// The factory of `function` the sidebar's units go to: the selected one (the lowest slot if
    /// several are selected), else the one with the lowest slot. `None` without one standing.
    pub fn primary(&self, function: BuildingFunction) -> Option<&OwnFactory> {
        let of_function = || self.factories.iter().filter(|f| f.function == function);
        of_function()
            .find(|f| f.selected)
            .or_else(|| of_function().next())
    }

    fn has_standing(&self, kind: Kind) -> bool {
        self.standing.binary_search(&kind).is_ok()
    }
}

/// Why an item that is being built does not advance.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Waiting {
    /// The player cannot pay the next tick.
    Credits,
    /// Finished, but no spot near the exit is free.
    Exit,
    /// The factory is stopped (the power hook of WP6.4).
    Stopped,
    /// A structure the unit needs no longer stands.
    Needs(Kind),
}

/// One item of a tab, as the sidebar draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// What it builds.
    pub kind: Kind,
    /// The name for people.
    pub name: String,
    /// The tab.
    pub tab: Tab,
    /// The price.
    pub cost: u32,
    /// Ticks to build.
    pub ticks: u32,
    /// The picture.
    pub picture: Picture,
    /// The player holds at least the price (always true without money rules).
    pub affordable: bool,
    /// Progress of the one being built, in thousandths; `None` when none of it is being built.
    pub progress: Option<u16>,
    /// How many of it are being built or wait in a queue.
    pub count: u8,
    /// Why it does not advance.
    pub waiting: Option<Waiting>,
    /// A unit: the factory a click goes to.
    pub factory: Option<EntityId>,
    /// A unit: the place in that queue a right-click takes out (the last of its kind; free items
    /// cannot be taken out).
    pub cancel_index: Option<u8>,
}

/// A mouse button on the sidebar.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// What a click on an item asks the game to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Choose this structure to place: the build mode of WP6.2 takes over (ghost on the cell under
    /// the cursor; its left click sends `BuildStructure`).
    ChooseStructure(Kind),
    /// Put the structure that is chosen away.
    PutAway,
    /// Send this command (`QueueUnit` or `CancelUnit`).
    Send(Command),
}

impl Item {
    /// What a click with `button` on this item does. A structure: left chooses it, right puts the
    /// chosen one away. A unit: left queues one, right takes the last one of its kind out of the
    /// queue (nothing when none is queued).
    pub fn click(&self, button: Button) -> Option<Action> {
        match (self.tab, button) {
            (Tab::Structures, Button::Left) => Some(Action::ChooseStructure(self.kind)),
            (Tab::Structures, Button::Right) => Some(Action::PutAway),
            (_, Button::Left) => self.factory.map(|factory| {
                Action::Send(Command::QueueUnit {
                    factory,
                    kind: self.kind,
                })
            }),
            (_, Button::Right) => match (self.factory, self.cancel_index) {
                (Some(factory), Some(index)) => {
                    Some(Action::Send(Command::CancelUnit { factory, index }))
                }
                _ => None,
            },
        }
    }
}

/// Whether the player can build `entry` now: every structure it needs stands and, for a unit, a
/// factory of its function stands. What is not available is not shown (`docs/rules/sidebar.md`).
pub fn available(entry: &Entry, sit: &Situation) -> bool {
    (entry.tab != Tab::Structures || sit.anchor)
        && entry.requires.iter().all(|&r| sit.has_standing(r))
        && entry
            .factory
            .is_none_or(|f| sit.factories.iter().any(|x| x.function == f))
}

/// The items `tab` shows: the entries of the catalog that are available, in catalog order.
pub fn items_for(catalog: &Catalog, sit: &Situation, tab: Tab) -> Vec<Item> {
    catalog
        .entries()
        .iter()
        .filter(|e| e.tab == tab && available(e, sit))
        .map(|e| item(e, sit))
        .collect()
}

/// How many items each tab shows, in tab order.
pub fn tab_counts(catalog: &Catalog, sit: &Situation) -> [usize; 4] {
    let mut counts = [0; 4];
    for e in catalog.entries() {
        if available(e, sit) {
            counts[e.tab.index()] += 1;
        }
    }
    counts
}

fn item(e: &Entry, sit: &Situation) -> Item {
    let mut it = Item {
        kind: e.kind,
        name: e.name.clone(),
        tab: e.tab,
        cost: e.cost,
        ticks: e.ticks,
        picture: e.picture.clone(),
        affordable: sit.credits.is_none_or(|c| c >= u64::from(e.cost)),
        progress: None,
        count: 0,
        waiting: None,
        factory: None,
        cancel_index: None,
    };
    match e.factory {
        None => {
            // A structure: the ones of this kind that are going up. The first (lowest slot) gives
            // the progress.
            let mine: Vec<&Building> = sit.buildings.iter().filter(|b| b.kind == e.kind).collect();
            it.count = u8::try_from(mine.len()).unwrap_or(u8::MAX);
            if let Some(first) = mine.first() {
                it.progress = Some(first.permille);
            }
            if mine.iter().any(|b| b.stalled) {
                it.waiting = Some(Waiting::Credits);
            }
        }
        Some(function) => {
            let Some(factory) = sit.primary(function) else {
                return it;
            };
            it.factory = Some(factory.id);
            let Some(view) = &factory.view else {
                return it;
            };
            let queued = view.queue.iter().filter(|q| q.kind == e.kind).count();
            it.count = u8::try_from(queued).unwrap_or(u8::MAX);
            it.cancel_index = view
                .queue
                .iter()
                .rposition(|q| q.kind == e.kind && !q.free)
                .and_then(|i| u8::try_from(i).ok());
            // The front item is the one that is being built; its state is the factory's.
            if view.queue.first().is_some_and(|q| q.kind == e.kind) {
                if view.front_ticks > 0 {
                    it.progress = Some(view.front_permille);
                }
                it.waiting = match view.status {
                    FactoryStatus::WaitingForMoney => Some(Waiting::Credits),
                    FactoryStatus::WaitingForExit => Some(Waiting::Exit),
                    FactoryStatus::Stopped => Some(Waiting::Stopped),
                    FactoryStatus::MissingPrerequisite(k) => Some(Waiting::Needs(k)),
                    FactoryStatus::Idle | FactoryStatus::Building | FactoryStatus::NotStanding => {
                        None
                    }
                };
            }
        }
    }
    it
}

/// The words for a waiting item: from `sidebar.ron`.
pub fn waiting_words(
    waiting: Waiting,
    look: &SidebarLook,
    name: &dyn Fn(Kind) -> String,
) -> String {
    match waiting {
        Waiting::Credits => look.words.waiting_credits.clone(),
        Waiting::Exit => look.words.exit_blocked.clone(),
        Waiting::Stopped => look.words.stopped.clone(),
        Waiting::Needs(k) => format!("{} {}", look.words.needs, name(k)),
    }
}

/// The power of a player, once the power rules exist (WP6.4).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PowerReading {
    /// Power made.
    pub made: u32,
    /// Power used.
    pub used: u32,
}

/// The hook WP6.4 fills: what the power bar shows. `None` while the world has no power rules (all
/// of today's): the bar then draws its neutral frame and says so.
pub fn power_reading(_snap: &SimSnapshot, _player: PlayerId) -> Option<PowerReading> {
    None
}

/// How full the power bar is, in thousandths, and whether it is in the low colour (more used than
/// made). It shows what is used against the larger of made and used.
pub fn power_fill(reading: PowerReading) -> (u16, bool) {
    let top = reading.made.max(reading.used).max(1);
    let permille = u64::from(reading.used) * 1000 / u64::from(top);
    (permille as u16, reading.used > reading.made)
}

/// Everything the sidebar draws in one frame: the items of the shown tab and the facts around them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarModel {
    /// The items of the tab that is shown.
    pub items: Vec<Item>,
    /// The words of each waiting item, parallel to `items`.
    pub waiting_words: Vec<Option<String>>,
    /// How many items each tab has, for dimming an empty tab.
    pub counts: [usize; 4],
    /// The credits of the player; `None` without money rules.
    pub credits: Option<u64>,
    /// Whether any structure or unit of the player waits for credits.
    pub waiting_for_credits: bool,
    /// The structure chosen to be placed (the build mode), if one is.
    pub chosen: Option<Kind>,
    /// The power of the player; `None` while there are no power rules.
    pub power: Option<PowerReading>,
    /// What an empty tab says; `None` while the tab has items.
    pub empty_hint: Option<String>,
}

/// Builds the model of `tab` for this frame.
pub fn model(
    catalog: &Catalog,
    sit: &Situation,
    tab: Tab,
    chosen: Option<Kind>,
    power: Option<PowerReading>,
    look: &SidebarLook,
) -> SidebarModel {
    let items = items_for(catalog, sit, tab);
    let waiting_words = items
        .iter()
        .map(|i| {
            i.waiting
                .map(|w| waiting_words(w, look, &|k| catalog.name_of(k)))
        })
        .collect();
    let waiting_for_credits = sit.buildings.iter().any(|b| b.stalled)
        || sit.factories.iter().any(|f| {
            f.view
                .as_ref()
                .is_some_and(|v| v.status == FactoryStatus::WaitingForMoney)
        });
    let empty_hint = items.is_empty().then(|| {
        let missing = match factory_of_tab(tab) {
            Some(f) => !sit.factories.iter().any(|x| x.function == f),
            None => !sit.anchor,
        };
        if missing {
            tab.pick(&look.empty_hints).clone()
        } else {
            look.words.nothing.clone()
        }
    });
    SidebarModel {
        items,
        waiting_words,
        empty_hint,
        counts: tab_counts(catalog, sit),
        credits: sit.credits,
        waiting_for_credits,
        chosen,
        power,
    }
}

#[cfg(test)]
mod tests;
