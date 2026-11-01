use super::*;
use crate::playfield::{with_buildings, with_harvest, with_storage};
use sf_data::units::{DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS, EMBEDDED_WEAPONS};
use sf_sim::{Cell, World, WorldConfig};

const ME: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);

struct Rig {
    world: World,
    defs: UnitDefs,
    catalog: Catalog,
    look: SidebarLook,
}

fn kind(defs: &UnitDefs, id: &str) -> Kind {
    Kind(defs.kind_of(id).unwrap())
}

/// A 64 x 64 world with the shipped rules: buildings, money (`credits` for Meridian) and production
/// with the given exit radius, and a construction centre of Meridian on (20, 20).
fn rig(credits: u64, exit_radius: u8) -> Rig {
    let defs = UnitDefs::embedded();
    let look = SidebarLook::embedded();
    let world = World::new(WorldConfig::new(7).with_map_size(64, 64))
        .unwrap()
        .with_units(sf_bridge::unit_table(&defs).unwrap())
        .unwrap();
    let world = with_storage(with_harvest(with_buildings(world, &defs), &defs), &defs);
    let world = world
        .with_money(
            sf_bridge::money_rules(&sf_data::money::MoneySettings::embedded()),
            &[(ME, credits), (THEM, 0)],
        )
        .unwrap();
    let settings = sf_data::production::ProductionSettings::from_ron(&format!(
        "(queue_limit: 5, exit_radius: {exit_radius})"
    ))
    .unwrap();
    let world = world
        .with_production(
            sf_bridge::production_rules(&settings),
            sf_bridge::production_table(&defs).unwrap(),
        )
        .unwrap();
    let catalog = Catalog::build(&defs, Faction::Meridian, &look);
    let mut rig = Rig {
        world,
        defs,
        catalog,
        look,
    };
    rig.place("construction_center", (20, 20));
    rig
}

impl Rig {
    fn send(&mut self, command: Command) {
        self.world
            .schedule(self.world.tick(), ME, command)
            .expect("the command is accepted");
    }

    /// Places a standing structure of Meridian and runs the tick that makes it.
    fn place(&mut self, id: &str, cell: (u32, u32)) {
        let kind = kind(&self.defs, id);
        let owner = ME;
        self.send(Command::PlaceStructure {
            kind,
            cell: Cell::new(cell.0, cell.1),
            owner,
        });
        self.world.step();
    }

    fn build(&mut self, id: &str, cell: (u32, u32)) {
        let kind = kind(&self.defs, id);
        self.send(Command::BuildStructure {
            kind,
            cell: Cell::new(cell.0, cell.1),
        });
    }

    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.world.step();
        }
    }

    fn snap(&self) -> SimSnapshot {
        SimSnapshot::capture(&self.world)
    }

    fn id_of(&self, id: &str) -> EntityId {
        let k = kind(&self.defs, id);
        self.snap()
            .views()
            .iter()
            .find(|v| v.kind == k && v.owner == ME)
            .unwrap_or_else(|| panic!("no {id}"))
            .id
    }

    fn sit(&self, selected: &[EntityId]) -> Situation {
        Situation::read(&self.snap(), ME, &self.catalog, selected)
    }

    fn items(&self, tab: Tab) -> Vec<Item> {
        items_for(&self.catalog, &self.sit(&[]), tab)
    }

    fn item(&self, tab: Tab, id: &str) -> Item {
        let k = kind(&self.defs, id);
        self.items(tab)
            .into_iter()
            .find(|i| i.kind == k)
            .unwrap_or_else(|| panic!("{id} is not on the tab"))
    }
}

fn names(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.name.as_str()).collect()
}

// ---- which items a tab shows ---------------------------------------------------------------------

#[test]
fn the_structures_tab_lists_what_the_standing_structures_allow() {
    let mut r = rig(1000, 4);
    // Only the construction centre stands: the power plant needs nothing else.
    assert_eq!(names(&r.items(Tab::Structures)), ["power plant"]);
    // With a power plant: the refinery and the barracks need it, in the order of the data.
    r.place("power_plant", (30, 20));
    assert_eq!(
        names(&r.items(Tab::Structures)),
        ["power plant", "refinery", "barracks"]
    );
    // The refinery opens the vehicle factory; the barracks opens the tower and the decoy.
    r.place("refinery", (30, 26));
    r.place("barracks", (36, 20));
    assert_eq!(
        names(&r.items(Tab::Structures)),
        [
            "power plant",
            "refinery",
            "barracks",
            "vehicle factory",
            "defence tower",
            "decoy emplacement"
        ]
    );
    // The radar, the shipyard and the advanced defences need the vehicle factory or each other.
    r.place("vehicle_factory", (36, 26));
    let tab = r.items(Tab::Structures);
    assert!(names(&tab).contains(&"radar") && names(&tab).contains(&"shipyard"));
    assert!(!names(&tab).contains(&"advanced defences"));
    // The structures of the other faction are not in the catalog at all.
    assert!(!names(&tab).contains(&"fortified bunker"));
}

#[test]
fn a_structure_that_is_still_going_up_does_not_count_as_standing() {
    let mut r = rig(1000, 4);
    r.build("power_plant", (26, 20));
    r.run(3);
    // The plant is under construction: the refinery and the barracks stay hidden.
    assert_eq!(names(&r.items(Tab::Structures)), ["power plant"]);
    // 15 s = 300 ticks: it stands, the next structures show.
    r.run(300);
    assert_eq!(
        names(&r.items(Tab::Structures)),
        ["power plant", "refinery", "barracks"]
    );
}

#[test]
fn a_unit_tab_needs_a_standing_factory_of_its_function() {
    let mut r = rig(1000, 4);
    r.place("power_plant", (30, 20));
    // No factory: nothing in the unit tabs, whatever the structures tab offers.
    for tab in [Tab::Infantry, Tab::Vehicles, Tab::Vessels] {
        assert!(r.items(tab).is_empty(), "{tab:?}");
    }
    // A barracks under construction does not count either.
    r.build("barracks", (26, 26));
    r.run(5);
    assert!(r.items(Tab::Infantry).is_empty());
    // Built (20 s = 400 ticks): the infantry of Meridian, in the order of their pictures. Only the
    // infantry tab: the vehicle factory is another function.
    r.run(400);
    assert_eq!(
        names(&r.items(Tab::Infantry)),
        ["rifleman", "rocket trooper", "engineer", "scout", "sniper"]
    );
    assert!(r.items(Tab::Vehicles).is_empty());
}

#[test]
fn the_units_of_a_tab_are_those_of_the_faction_in_the_order_of_the_data() {
    let mut r = rig(1000, 4);
    r.place("power_plant", (30, 20));
    r.place("refinery", (30, 26));
    r.place("barracks", (36, 20));
    r.place("vehicle_factory", (36, 26));
    // Meridian: no medic (Veyran), a sniper (Meridian only). The order is the `order` of the
    // build pictures: rifleman 10, rocket trooper 20, engineer 30, scout 40, sniper 50.
    assert_eq!(
        names(&r.items(Tab::Infantry)),
        ["rifleman", "rocket trooper", "engineer", "scout", "sniper"]
    );
    assert_eq!(
        names(&r.items(Tab::Vehicles)),
        [
            "harvester",
            "light tank",
            "heavy tank",
            "siege artillery",
            "repair truck",
            "light transport",
            "amphibious transport"
        ]
    );
    // Veyran's catalog of the same world is another list: a medic, no sniper, a gunboat.
    let veyran = Catalog::build(&r.defs, Faction::Veyran, &r.look);
    let ids = |tab| {
        veyran
            .entries()
            .iter()
            .filter(|e| e.tab == tab)
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>()
    };
    assert!(ids(Tab::Infantry).contains(&"medic"));
    assert!(!ids(Tab::Infantry).contains(&"sniper"));
    assert_eq!(ids(Tab::Vessels), ["gunboat"]);
    assert!(r.catalog.entries().iter().all(|e| e.tab != Tab::Vessels));
}

#[test]
fn the_order_follows_the_order_of_the_pictures_not_the_kind_numbers() {
    // Swap the places of the rifleman (kind 12) and the scout (kind 15): the list follows.
    let mut text = EMBEDDED_UNITS.replace("\r\n", "\n");
    text = text
        .replacen(
            r#"label: "RIFLE", order: 10)"#,
            r#"label: "RIFLE", order: 41)"#,
            1,
        )
        .replacen(
            r#"label: "SCOUT", order: 40)"#,
            r#"label: "SCOUT", order: 5)"#,
            1,
        );
    let defs = defs_from(&text);
    let catalog = Catalog::build(&defs, Faction::Meridian, &SidebarLook::embedded());
    let infantry: Vec<&str> = catalog
        .entries()
        .iter()
        .filter(|e| e.tab == Tab::Infantry)
        .map(|e| e.id.as_str())
        .collect();
    assert_eq!(
        infantry,
        ["scout", "rocket_trooper", "engineer", "rifleman", "sniper"]
    );
}

fn defs_from(units: &str) -> UnitDefs {
    let (c, w) = (
        EMBEDDED_CLASSES.replace("\r\n", "\n"),
        EMBEDDED_WEAPONS.replace("\r\n", "\n"),
    );
    UnitDefs::from_sources(
        &DefSources {
            classes: DefFile {
                name: "classes.ron",
                text: &c,
            },
            weapons: DefFile {
                name: "weapons.ron",
                text: &w,
            },
            units: DefFile {
                name: "units.ron",
                text: units,
            },
        },
        None,
    )
    .expect("the changed definitions load")
}

#[test]
fn a_unit_with_prerequisites_waits_for_them() {
    // The data says a rifleman needs a radar (the shipped data has no unit prerequisites yet).
    let text = EMBEDDED_UNITS.replace("\r\n", "\n").replacen(
        r#"kind: 12, id: "rifleman", factions: [Meridian, Veyran], category: Infantry,"#,
        r#"kind: 12, id: "rifleman", factions: [Meridian, Veyran], category: Infantry, requires: ["radar"],"#,
        1,
    );
    let defs = defs_from(&text);
    let catalog = Catalog::build(&defs, Faction::Meridian, &SidebarLook::embedded());
    let rifleman = catalog.entry(kind(&defs, "rifleman")).unwrap();
    assert_eq!(rifleman.requires, [kind(&defs, "radar")]);
    let barracks = OwnFactory {
        id: EntityId::new(3, 0),
        function: BuildingFunction::InfantryFactory,
        selected: false,
        view: None,
    };
    let mut sit = Situation {
        standing: vec![kind(&defs, "barracks")],
        factories: vec![barracks],
        ..Situation::default()
    };
    let shown = |sit: &Situation| names(&items_for(&catalog, sit, Tab::Infantry)).join(",");
    assert_eq!(shown(&sit), "rocket trooper,engineer,scout,sniper");
    // The radar stands: the rifleman is back, first, because of its picture's order.
    sit.standing = vec![kind(&defs, "radar"), kind(&defs, "barracks")];
    sit.standing.sort();
    assert_eq!(shown(&sit), "rifleman,rocket trooper,engineer,scout,sniper");
}

#[test]
fn a_role_without_a_picture_gets_the_default_of_its_tab_and_comes_last() {
    let text = EMBEDDED_UNITS.replace("\r\n", "\n").replacen(
        r#"build_picture: (color: (90, 170, 100), label: "RIFLE", order: 10),"#,
        "",
        1,
    );
    let defs = defs_from(&text);
    let look = SidebarLook::embedded();
    let catalog = Catalog::build(&defs, Faction::Meridian, &look);
    let infantry: Vec<&Entry> = catalog
        .entries()
        .iter()
        .filter(|e| e.tab == Tab::Infantry)
        .collect();
    let rifleman = infantry.last().unwrap();
    assert_eq!(rifleman.id, "rifleman");
    // The first letters of the id, capitals, in the colour of the tab.
    assert_eq!(rifleman.picture.label, "RIFL");
    assert_eq!(rifleman.picture.color, look.default_colors.1);
    assert_eq!(rifleman.order, 1000 + 12);
    // The others keep their own pictures.
    assert_eq!(infantry[0].picture.label, "ROCKT");
}

// ---- prices, times and pictures are the data's ---------------------------------------------------

#[test]
fn every_entry_has_the_price_and_time_the_simulation_will_charge() {
    let r = rig(1000, 4);
    let snap = r.snap();
    let mut checked = 0;
    for e in r.catalog.entries() {
        let (cost, ticks) = match e.factory {
            Some(_) => snap.menu().price_of(e.kind).expect("a unit of the table"),
            None => {
                let b = r.world.building_table().get(e.kind).expect("a structure");
                (b.cost.expect("a price"), b.build_ticks)
            }
        };
        assert_eq!((e.cost, e.ticks), (cost, ticks), "{}", e.id);
        // The picture and the tab are the record's.
        let def = r.defs.unit(&e.id).unwrap();
        let p = def.build_picture.as_ref().unwrap();
        assert_eq!(
            (e.picture.label.as_str(), e.picture.color),
            (p.label.as_str(), p.color)
        );
        assert_eq!(e.order, u32::from(p.order));
        assert_eq!(Some(e.tab), Tab::of_category(def.category), "{}", e.id);
        checked += 1;
    }
    // The ten structures of Meridian less the bunker of Veyran, plus its infantry, vehicles.
    assert_eq!(checked, 9 + 5 + 7);
    // The data tells the entries apart, so that a mixed-up mapping cannot hide: of the 21 entries at
    // least 20 have a price and a time of their own (the radar and the advanced defences share
    // theirs, and are told apart by their pictures above).
    let mut pairs: Vec<(u32, u32)> = r
        .catalog
        .entries()
        .iter()
        .map(|e| (e.cost, e.ticks))
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    assert!(
        pairs.len() >= 20,
        "{} distinct prices and times",
        pairs.len()
    );
}

#[test]
fn the_factory_of_a_unit_is_the_one_the_data_derives_from_its_category() {
    for c in [
        Category::Infantry,
        Category::Vehicle,
        Category::Vessel,
        Category::Structure,
        Category::Object,
    ] {
        let data = sf_data::units::produced_by(c).map(|f| f.number());
        assert_eq!(factory_for(c).map(|f| f.number()), data, "{c:?}");
    }
    assert_eq!(Tab::of_category(Category::Object), None);
    for (i, t) in Tab::ALL.into_iter().enumerate() {
        assert_eq!(t.index(), i);
    }
}

// ---- progress, the queue and what waits ----------------------------------------------------------

/// A world with a power plant, a refinery and a barracks standing; the barracks on (36, 20).
fn with_barracks(credits: u64, exit_radius: u8) -> Rig {
    let mut r = rig(credits, exit_radius);
    r.place("power_plant", (30, 20));
    r.place("refinery", (30, 26));
    r.place("barracks", (36, 20));
    r
}

#[test]
fn a_queued_unit_shows_its_own_progress_and_count() {
    let mut r = with_barracks(5000, 4);
    let barracks = r.id_of("barracks");
    let (rifleman, rocket) = (kind(&r.defs, "rifleman"), kind(&r.defs, "rocket_trooper"));
    for k in [rifleman, rocket, rifleman] {
        r.send(Command::QueueUnit {
            factory: barracks,
            kind: k,
        });
    }
    r.run(30);
    let view = r.snap().factory_of(barracks).cloned().expect("a queue");
    assert_eq!(view.queue.len(), 3);
    let a = r.item(Tab::Infantry, "rifleman");
    let b = r.item(Tab::Infantry, "rocket_trooper");
    // The rifleman is at the front and has progress, twice in the queue; the rocket trooper
    // waits behind it with none. The numbers are the simulation's.
    assert_eq!((a.count, b.count), (2, 1));
    assert_eq!(a.progress, Some(view.front_permille));
    assert!(view.front_permille > 0);
    assert_eq!(b.progress, None);
    assert_eq!((a.waiting, b.waiting), (None, None));
    // An item that is not queued has neither.
    let scout = r.item(Tab::Infantry, "scout");
    assert_eq!((scout.count, scout.progress), (0, None));
    // A right click takes the last of its kind out: place 2 for the rifleman, 1 for the rocket.
    assert_eq!(a.cancel_index, Some(2));
    assert_eq!(b.cancel_index, Some(1));
    assert_eq!(scout.cancel_index, None);
}

#[test]
fn a_structure_going_up_shows_its_own_progress_and_count() {
    let mut r = rig(5000, 4);
    r.build("power_plant", (26, 20));
    r.run(100);
    r.build("power_plant", (26, 26));
    r.run(40);
    let item = r.item(Tab::Structures, "power_plant");
    // Two plants are going up; the first (older, further along) gives the progress.
    assert_eq!(item.count, 2);
    let snap = r.snap();
    let first = snap
        .constructions()
        .iter()
        .map(|(id, c)| (id.index(), c.permille()))
        .min()
        .unwrap();
    assert_eq!(item.progress, Some(first.1));
    assert!(
        first.1 > 400,
        "the older plant is well on its way: {first:?}"
    );
    assert_eq!(item.waiting, None);
}

#[test]
fn a_structure_that_waits_for_credits_says_so() {
    // 100 credits for a plant that costs 300: the build pauses when they are gone.
    let mut r = rig(100, 4);
    r.build("power_plant", (26, 20));
    r.run(200);
    let item = r.item(Tab::Structures, "power_plant");
    assert_eq!(item.waiting, Some(Waiting::Credits));
    assert!(item.progress.is_some());
    assert!(!item.affordable, "0 credits left for a price of 300");
    let sit = r.sit(&[]);
    let model = model(&r.catalog, &sit, Tab::Structures, None, None, &r.look);
    assert!(model.waiting_for_credits);
    assert_eq!(model.waiting_words[0].as_deref(), Some("no credits"));
}

#[test]
fn a_unit_that_waits_for_credits_says_so_on_the_item_at_the_front_only() {
    let mut r = with_barracks(150, 4);
    let barracks = r.id_of("barracks");
    let (rocket, rifleman) = (kind(&r.defs, "rocket_trooper"), kind(&r.defs, "rifleman"));
    for k in [rocket, rifleman] {
        r.send(Command::QueueUnit {
            factory: barracks,
            kind: k,
        });
    }
    // The refinery's free harvester does not matter; 150 credits cannot pay 300 for the trooper.
    r.run(200);
    let view = r.snap().factory_of(barracks).cloned().unwrap();
    assert_eq!(view.status, FactoryStatus::WaitingForMoney);
    let (front, behind) = (
        r.item(Tab::Infantry, "rocket_trooper"),
        r.item(Tab::Infantry, "rifleman"),
    );
    assert_eq!(front.waiting, Some(Waiting::Credits));
    assert_eq!(
        behind.waiting, None,
        "the one behind the front only waits its turn"
    );
    assert!(front.progress.is_some() && behind.progress.is_none());
    assert_eq!((front.count, behind.count), (1, 1));
}

#[test]
fn a_finished_unit_with_no_free_exit_says_the_exit_is_blocked() {
    // The barracks stands on the east edge of the map, so that no cell round its exit exists; the
    // exit radius is 1 (the pinned scenario does the same with a power plant).
    let mut r = rig(5000, 1);
    r.place("power_plant", (30, 20));
    r.place("barracks", (62, 30));
    let barracks = r.id_of("barracks");
    r.send(Command::QueueUnit {
        factory: barracks,
        kind: kind(&r.defs, "rifleman"),
    });
    // 5 s = 100 ticks.
    r.run(140);
    let view = r.snap().factory_of(barracks).cloned().unwrap();
    assert_eq!(view.status, FactoryStatus::WaitingForExit);
    let item = r.item(Tab::Infantry, "rifleman");
    assert_eq!(item.waiting, Some(Waiting::Exit));
    let sit = r.sit(&[]);
    let m = model(&r.catalog, &sit, Tab::Infantry, None, None, &r.look);
    assert_eq!(m.waiting_words[0].as_deref(), Some("exit blocked"));
    // Not a credit problem: the sidebar does not call it one.
    assert!(!m.waiting_for_credits);
}

#[test]
fn the_words_of_a_wait_come_from_the_look() {
    let look = SidebarLook::embedded();
    let name = |k: Kind| format!("kind{}", k.0);
    assert_eq!(waiting_words(Waiting::Credits, &look, &name), "no credits");
    assert_eq!(waiting_words(Waiting::Exit, &look, &name), "exit blocked");
    assert_eq!(waiting_words(Waiting::Stopped, &look, &name), "stopped");
    assert_eq!(
        waiting_words(Waiting::Needs(Kind(4)), &look, &name),
        "needs kind4"
    );
}

#[test]
fn a_price_the_player_cannot_pay_is_marked_but_the_item_stays() {
    let r = with_barracks(250, 4);
    // 250 credits after nothing was spent (placed structures are free): the trooper costs 300.
    let rifleman = r.item(Tab::Infantry, "rifleman");
    let trooper = r.item(Tab::Infantry, "rocket_trooper");
    assert!(rifleman.affordable && !trooper.affordable);
    // Without money rules everything is affordable.
    let sit = Situation {
        credits: None,
        ..r.sit(&[])
    };
    assert!(
        items_for(&r.catalog, &sit, Tab::Infantry)
            .iter()
            .all(|i| i.affordable)
    );
}

// ---- which factory a click goes to ---------------------------------------------------------------

#[test]
fn the_primary_factory_is_the_selected_one_else_the_lowest_slot() {
    let mut r = with_barracks(5000, 4);
    r.place("barracks", (36, 32));
    r.place("vehicle_factory", (44, 26));
    let snap = r.snap();
    let barracks_kind = kind(&r.defs, "barracks");
    let mut both: Vec<EntityId> = snap
        .views()
        .iter()
        .filter(|v| v.kind == barracks_kind)
        .map(|v| v.id)
        .collect();
    both.sort_by_key(|id| id.index());
    let (low, high) = (both[0], both[1]);
    let rifleman = r.item(Tab::Infantry, "rifleman");
    // Nothing selected: the lowest slot.
    assert_eq!(rifleman.factory, Some(low));
    // The second barracks selected: it gets the clicks.
    let sit = r.sit(&[high]);
    let item = items_for(&r.catalog, &sit, Tab::Infantry)
        .into_iter()
        .find(|i| i.name == "rifleman")
        .unwrap();
    assert_eq!(item.factory, Some(high));
    assert_eq!(
        item.click(Button::Left),
        Some(Action::Send(Command::QueueUnit {
            factory: high,
            kind: item.kind
        }))
    );
    // Selecting the vehicle factory does not move the infantry: the function must match.
    let vf = r.id_of("vehicle_factory");
    let sit = r.sit(&[vf]);
    let item = items_for(&r.catalog, &sit, Tab::Infantry)
        .into_iter()
        .find(|i| i.name == "rifleman")
        .unwrap();
    assert_eq!(item.factory, Some(low));
    // And each tab has its own primary.
    let tank = items_for(&r.catalog, &sit, Tab::Vehicles)
        .into_iter()
        .find(|i| i.name == "light tank")
        .unwrap();
    assert_eq!(tank.factory, Some(vf));
}

// ---- what a click does ---------------------------------------------------------------------------

#[test]
fn a_click_on_a_structure_chooses_it_and_a_right_click_puts_it_away() {
    let mut r = rig(1000, 4);
    r.place("power_plant", (30, 20));
    let refinery = r.item(Tab::Structures, "refinery");
    assert_eq!(
        refinery.click(Button::Left),
        Some(Action::ChooseStructure(kind(&r.defs, "refinery")))
    );
    assert_eq!(refinery.click(Button::Right), Some(Action::PutAway));
    // Different structures choose different kinds.
    let plant = r.item(Tab::Structures, "power_plant");
    assert_ne!(plant.click(Button::Left), refinery.click(Button::Left));
}

#[test]
fn a_click_on_a_unit_queues_it_and_a_right_click_cancels_the_last_one_of_its_kind() {
    let mut r = with_barracks(5000, 4);
    let barracks = r.id_of("barracks");
    let (rifleman, rocket) = (kind(&r.defs, "rifleman"), kind(&r.defs, "rocket_trooper"));
    let queue_of = |r: &Rig| -> Vec<Kind> {
        r.snap()
            .factory_of(barracks)
            .map(|v| v.queue.iter().map(|q| q.kind).collect())
            .unwrap_or_default()
    };
    // Nothing queued: a right click does nothing.
    assert_eq!(r.item(Tab::Infantry, "rifleman").click(Button::Right), None);
    // Left clicks, run as the commands they are, fill the queue.
    for id in ["rifleman", "rocket_trooper", "rifleman"] {
        let Some(Action::Send(command)) = r.item(Tab::Infantry, id).click(Button::Left) else {
            panic!("a unit click is a command");
        };
        r.send(command);
        r.run(2);
    }
    assert_eq!(queue_of(&r), [rifleman, rocket, rifleman]);
    // A right click on the rifleman takes the last rifleman (place 2) out, not the first.
    let Some(Action::Send(command)) = r.item(Tab::Infantry, "rifleman").click(Button::Right) else {
        panic!("a cancel is a command");
    };
    assert_eq!(
        command,
        Command::CancelUnit {
            factory: barracks,
            index: 2
        }
    );
    r.send(command);
    r.run(2);
    assert_eq!(queue_of(&r), [rifleman, rocket]);
    // And on the rocket trooper: place 1.
    let Some(Action::Send(command)) = r.item(Tab::Infantry, "rocket_trooper").click(Button::Right)
    else {
        panic!("a cancel is a command");
    };
    r.send(command);
    r.run(2);
    assert_eq!(queue_of(&r), [rifleman]);
}

#[test]
fn the_free_harvester_of_a_refinery_cannot_be_cancelled_from_the_sidebar() {
    // A refinery that is built brings a harvester at the head of the vehicle queue, free.
    let mut r = rig(5000, 4);
    r.place("power_plant", (30, 20));
    r.place("vehicle_factory", (36, 26));
    r.build("refinery", (30, 26));
    r.run(850); // 40 s = 800 ticks
    let vf = r.id_of("vehicle_factory");
    let view = r.snap().factory_of(vf).cloned();
    let free = view
        .as_ref()
        .is_some_and(|v| v.queue.iter().any(|q| q.free));
    let harvester = r.item(Tab::Vehicles, "harvester");
    if free {
        // Counted in the queue, but nothing a right click can take out.
        assert_eq!(harvester.count, 1);
        assert_eq!(harvester.cancel_index, None);
        assert_eq!(harvester.click(Button::Right), None);
    } else {
        // It already rolled out (nothing in the way): then there is nothing to show.
        assert_eq!(harvester.count, 0);
    }
}

#[test]
fn a_click_on_a_unit_without_a_factory_does_nothing() {
    // An item built by hand with no factory (it cannot be on a tab, but the click must not panic).
    let r = with_barracks(1000, 4);
    let mut item = r.item(Tab::Infantry, "rifleman");
    item.factory = None;
    assert_eq!(item.click(Button::Left), None);
    assert_eq!(item.click(Button::Right), None);
}

// ---- the model -----------------------------------------------------------------------------------

#[test]
fn the_tab_counts_say_which_tabs_are_empty() {
    let mut r = rig(1000, 4);
    r.place("power_plant", (30, 20));
    let c = tab_counts(&r.catalog, &r.sit(&[]));
    assert_eq!(c, [3, 0, 0, 0]);
    r.place("barracks", (36, 20));
    let c = tab_counts(&r.catalog, &r.sit(&[]));
    assert_eq!(c[Tab::Infantry.index()], 5);
    assert_eq!(c[Tab::Vehicles.index()], 0);
    assert_eq!(c[Tab::Vessels.index()], 0);
}

#[test]
fn the_model_carries_the_chosen_structure_and_the_credits() {
    let r = rig(777, 4);
    let sit = r.sit(&[]);
    let chosen = Some(kind(&r.defs, "power_plant"));
    let m = model(&r.catalog, &sit, Tab::Structures, chosen, None, &r.look);
    assert_eq!(m.chosen, chosen);
    assert_eq!(m.credits, Some(777));
    assert_eq!(m.power, None);
    assert_eq!(m.items.len(), m.waiting_words.len());
    assert!(!m.waiting_for_credits);
}

#[test]
fn only_the_local_players_structures_count() {
    // A power plant of the other player does not open the refinery for us.
    let mut r = rig(1000, 4);
    let plant = kind(&r.defs, "power_plant");
    r.world
        .schedule(
            r.world.tick(),
            THEM,
            Command::PlaceStructure {
                kind: plant,
                cell: Cell::new(40, 40),
                owner: THEM,
            },
        )
        .unwrap();
    r.world.step();
    assert_eq!(names(&r.items(Tab::Structures)), ["power plant"]);
}

// ---- power ---------------------------------------------------------------------------------------

#[test]
fn the_power_hook_reads_nothing_until_the_power_rules_exist() {
    let r = rig(1000, 4);
    assert_eq!(power_reading(&r.snap(), ME), None);
}

#[test]
fn the_power_bar_fills_with_what_is_used_against_what_is_made() {
    assert_eq!(
        power_fill(PowerReading {
            made: 100,
            used: 25
        }),
        (250, false)
    );
    assert_eq!(
        power_fill(PowerReading {
            made: 100,
            used: 100
        }),
        (1000, false)
    );
    // More used than made: full and in the low colour.
    assert_eq!(
        power_fill(PowerReading {
            made: 50,
            used: 100
        }),
        (1000, true)
    );
    // Nothing made, nothing used: empty, not a division by zero.
    assert_eq!(power_fill(PowerReading { made: 0, used: 0 }), (0, false));
    assert_eq!(power_fill(PowerReading { made: 0, used: 40 }), (1000, true));
}
