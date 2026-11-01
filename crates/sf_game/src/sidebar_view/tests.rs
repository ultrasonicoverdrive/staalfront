use super::*;
use crate::sidebar::{Catalog, OwnFactory, PowerReading, Situation, Waiting, model as build_model};
use sf_data::money_look::MoneyLook;
use sf_data::units::{Faction, UnitDefs};
use sf_sim::{BuildingFunction, Command, EntityId, Kind};
use sf_ui::{MouseButton, UiConfig};

const BARRACKS_SLOT: EntityId = EntityId::new(3, 0);

struct Fixture {
    defs: UnitDefs,
    catalog: Catalog,
    look: SidebarLook,
    money: MoneyLook,
}

fn fixture() -> Fixture {
    let defs = UnitDefs::embedded();
    let look = SidebarLook::embedded();
    let catalog = Catalog::build(&defs, Faction::Meridian, &look);
    Fixture {
        defs,
        catalog,
        look,
        money: MoneyLook::embedded(),
    }
}

impl Fixture {
    fn kind(&self, id: &str) -> Kind {
        Kind(self.defs.kind_of(id).unwrap())
    }

    /// Everything a player can have standing, with one barracks.
    fn situation(&self, credits: Option<u64>) -> Situation {
        let mut standing: Vec<Kind> = [
            "construction_center",
            "power_plant",
            "refinery",
            "barracks",
            "vehicle_factory",
            "radar",
            "defence_tower",
            "shipyard",
        ]
        .iter()
        .map(|id| self.kind(id))
        .collect();
        standing.sort();
        Situation {
            standing,
            factories: vec![OwnFactory {
                id: BARRACKS_SLOT,
                function: BuildingFunction::InfantryFactory,
                selected: false,
                view: None,
            }],
            buildings: Vec::new(),
            credits,
        }
    }

    fn model(&self, sit: &Situation, tab: Tab) -> SidebarModel {
        build_model(&self.catalog, sit, tab, None, None, &self.look)
    }

    fn setup(&self) -> Setup<'_> {
        Setup {
            look: &self.look,
            money: &self.money,
            edge_band_px: 4.0,
            minimap: true,
        }
    }
}

fn at(x: f32, y: f32) -> UiInput {
    UiInput {
        cursor: Some((x, y)),
        ..UiInput::default()
    }
}

fn press(mut input: UiInput, button: MouseButton) -> UiInput {
    input.pressed[button as usize] = true;
    input.down[button as usize] = true;
    input
}

struct Frame {
    ui: Ui,
    actions: Vec<Action>,
}

/// Runs one frame of the sidebar in a 1280 x 720 window at scale 1.
fn run(
    f: &Fixture,
    state: &mut SidebarState,
    model: &SidebarModel,
    input: UiInput,
    size: (u32, u32),
) -> Frame {
    let mut ui = Ui::new(UiConfig::default());
    ui.begin_frame(input, size, 1.0);
    let actions = frame(&mut ui, &input, state, model, &f.setup());
    ui.end_frame();
    Frame { ui, actions }
}

fn lay(f: &Fixture, size: (u32, u32)) -> Layout {
    sidebar_layout::layout(
        Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
        1.0,
        &f.look,
        4.0,
        true,
    )
}

const SIZE: (u32, u32) = (1280, 720);

#[test]
fn a_press_on_a_structure_chooses_it_and_the_ui_keeps_the_click() {
    let f = fixture();
    let model = f.model(&f.situation(Some(1000)), Tab::Structures);
    let l = lay(&f, SIZE);
    let (x, y) = l.slot(1).center();
    let mut state = SidebarState::default();
    let out = run(
        &f,
        &mut state,
        &model,
        press(at(x, y), MouseButton::Left),
        SIZE,
    );
    // Slot 1 is the second structure: the refinery.
    assert_eq!(out.actions, [Action::ChooseStructure(f.kind("refinery"))]);
    // The click is the UI's: no selection box and no order behind it.
    assert!(out.ui.pointer_over_ui());
    assert!(out.ui.press_consumed(MouseButton::Left));
    assert!(out.ui.button_captured(MouseButton::Left));
    // The map would have seen it without the sidebar: the same press elsewhere is not consumed.
    let out = run(
        &f,
        &mut state,
        &model,
        press(at(300.0, 300.0), MouseButton::Left),
        SIZE,
    );
    assert!(out.actions.is_empty());
    assert!(!out.ui.pointer_over_ui());
    assert!(!out.ui.press_consumed(MouseButton::Left));
}

#[test]
fn a_right_press_on_a_structure_puts_the_chosen_one_away_and_orders_nothing() {
    let f = fixture();
    let model = f.model(&f.situation(Some(1000)), Tab::Structures);
    let (x, y) = lay(&f, SIZE).slot(0).center();
    let mut state = SidebarState::default();
    let out = run(
        &f,
        &mut state,
        &model,
        press(at(x, y), MouseButton::Right),
        SIZE,
    );
    assert_eq!(out.actions, [Action::PutAway]);
    assert!(out.ui.press_consumed(MouseButton::Right));
}

#[test]
fn a_press_on_a_unit_queues_it_and_a_right_press_cancels_one() {
    let f = fixture();
    let mut state = SidebarState {
        tab: Tab::Infantry,
        scroll: 0,
    };
    let sit = f.situation(Some(1000));
    let model = f.model(&sit, Tab::Infantry);
    assert_eq!(model.items[0].name, "rifleman");
    let (x, y) = lay(&f, SIZE).slot(0).center();
    let out = run(
        &f,
        &mut state,
        &model,
        press(at(x, y), MouseButton::Left),
        SIZE,
    );
    assert_eq!(
        out.actions,
        [Action::Send(Command::QueueUnit {
            factory: BARRACKS_SLOT,
            kind: f.kind("rifleman")
        })]
    );
    // With nothing queued a right press only keeps the click from the map.
    let out = run(
        &f,
        &mut state,
        &model,
        press(at(x, y), MouseButton::Right),
        SIZE,
    );
    assert!(out.actions.is_empty());
    assert!(out.ui.press_consumed(MouseButton::Right));
}

#[test]
fn a_press_on_a_tab_shows_it_and_a_right_press_does_not() {
    let f = fixture();
    let sit = f.situation(Some(1000));
    let model = f.model(&sit, Tab::Structures);
    let l = lay(&f, SIZE);
    let mut state = SidebarState {
        tab: Tab::Structures,
        scroll: 1,
    };
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let (x, y) = l.tabs[i].center();
        let out = run(
            &f,
            &mut state,
            &model,
            press(at(x, y), MouseButton::Left),
            SIZE,
        );
        assert!(out.actions.is_empty());
        assert_eq!(state.tab, tab, "tab {i}");
        assert_eq!(state.scroll, 0);
        assert!(out.ui.press_consumed(MouseButton::Left));
    }
    state.tab = Tab::Structures;
    let (x, y) = l.tabs[2].center();
    run(
        &f,
        &mut state,
        &model,
        press(at(x, y), MouseButton::Right),
        SIZE,
    );
    assert_eq!(state.tab, Tab::Structures);
}

#[test]
fn a_press_on_the_column_between_the_parts_is_consumed_and_does_nothing() {
    let f = fixture();
    let model = f.model(&f.situation(Some(1000)), Tab::Structures);
    let l = lay(&f, SIZE);
    let mut state = SidebarState::default();
    for (x, y) in [
        l.money.center(),
        l.power.center(),
        // Between two tiles, and below the last one.
        (l.slot(0).right() + l.gap * 0.5, l.slot(0).center().1),
        l.slot(9).center(),
    ] {
        let out = run(
            &f,
            &mut state,
            &model,
            press(at(x, y), MouseButton::Left),
            SIZE,
        );
        assert!(out.actions.is_empty(), "({x}, {y})");
        assert!(out.ui.press_consumed(MouseButton::Left), "({x}, {y})");
    }
}

#[test]
fn the_edge_band_and_the_sidebar_do_not_fight() {
    let f = fixture();
    let model = f.model(&f.situation(Some(1000)), Tab::Structures);
    let l = lay(&f, SIZE);
    let mut state = SidebarState::default();
    // The last pixel columns of the window (the band of 4) are not the sidebar's: the UI does not
    // have the pointer there, so the camera scrolls and the cursor shows the scroll arrow.
    let y = l.slot(0).center().1;
    for x in [1276.0, 1277.0, 1279.0] {
        let out = run(
            &f,
            &mut state,
            &model,
            press(at(x, y), MouseButton::Left),
            SIZE,
        );
        assert!(!out.ui.pointer_over_ui(), "x = {x}");
        assert!(out.actions.is_empty());
    }
    // And the last pixel column of the sidebar (it ends at 1274, 6 from the edge) is the sidebar's.
    let out = run(&f, &mut state, &model, at(1273.0, y), SIZE);
    assert!(out.ui.pointer_over_ui());
}

#[test]
fn the_wheel_scrolls_the_items_over_the_column_only() {
    let f = fixture();
    // Everything standing and a window too short for all nine structures: 2 rows of 2 fit.
    let sit = f.situation(Some(1000));
    let model = f.model(&sit, Tab::Structures);
    let size = (1280, 500);
    let l = lay(&f, size);
    assert_eq!(l.rows_visible, 2, "{l:?}");
    assert!(model.items.len() > l.visible_slots());
    let mut state = SidebarState::default();
    let mut wheel = at(l.slot(0).center().0, l.slot(0).center().1);
    wheel.wheel = -1.0;
    let out = run(&f, &mut state, &model, wheel, size);
    assert_eq!(state.scroll, 1);
    assert!(
        out.ui.wheel_consumed(),
        "the wheel over the sidebar is not a zoom"
    );
    // Slot 0 is now the third item.
    let (x, y) = l.slot(0).center();
    let out = run(
        &f,
        &mut state,
        &model,
        press(at(x, y), MouseButton::Left),
        size,
    );
    assert_eq!(out.actions, [Action::ChooseStructure(model.items[2].kind)]);
    // The wheel elsewhere does not scroll.
    let mut elsewhere = at(300.0, 300.0);
    elsewhere.wheel = -1.0;
    let mut other = SidebarState::default();
    run(&f, &mut other, &model, elsewhere, size);
    assert_eq!(other.scroll, 0);
    // Up again.
    let mut up = at(x, y);
    up.wheel = 2.0;
    run(&f, &mut state, &model, up, size);
    assert_eq!(state.scroll, 0);
}

#[test]
fn a_scroll_past_the_end_after_the_list_shrinks_is_pulled_back() {
    let f = fixture();
    let size = (1280, 500);
    let mut state = SidebarState {
        tab: Tab::Structures,
        scroll: 3,
    };
    // Two items: nothing to scroll.
    let model = f.model(&Situation::default(), Tab::Structures);
    assert!(model.items.len() <= 2);
    run(&f, &mut state, &model, UiInput::default(), size);
    assert_eq!(state.scroll, 0);
}

// ---- what is drawn -------------------------------------------------------------------------------

fn color_array(c: (u8, u8, u8, u8)) -> [u8; 4] {
    [c.0, c.1, c.2, c.3]
}

/// The quads of the draw list in `color`, as rectangles.
fn quads_in(ui: &Ui, color: [u8; 4]) -> Vec<Rect> {
    ui.draw_list()
        .vertices
        .chunks(4)
        .filter(|q| q.len() == 4 && q.iter().all(|v| v.color == color))
        .map(|q| {
            Rect::new(
                q[0].pos[0],
                q[0].pos[1],
                q[2].pos[0] - q[0].pos[0],
                q[2].pos[1] - q[0].pos[1],
            )
        })
        .collect()
}

#[test]
fn every_item_is_drawn_with_the_colour_of_its_own_picture() {
    let f = fixture();
    let model = f.model(&f.situation(Some(1000)), Tab::Structures);
    let l = lay(&f, SIZE);
    let mut state = SidebarState::default();
    let out = run(&f, &mut state, &model, at(10.0, 10.0), SIZE);
    for (n, item) in model.items.iter().enumerate().take(l.visible_slots()) {
        let c = item.picture.color;
        let quads = quads_in(&out.ui, [c.0, c.1, c.2, 255]);
        let slot = l.slot(n);
        // The picture of the item lies inside its own slot and in no other.
        assert_eq!(quads.len(), 1, "{}", item.name);
        let q = quads[0];
        assert!(
            q.x >= slot.x
                && q.right() <= slot.right()
                && q.y >= slot.y
                && q.bottom() <= slot.bottom(),
            "{}: {q:?} not in slot {n} {slot:?}",
            item.name
        );
    }
}

#[test]
fn the_progress_bar_is_as_long_as_the_progress_and_turns_to_the_waiting_colour() {
    let f = fixture();
    let mut model = f.model(&f.situation(Some(1000)), Tab::Structures);
    // Item 0 is 40 % along, item 1 is 75 % along and waits for credits.
    model.items[0].progress = Some(400);
    model.items[1].progress = Some(750);
    model.items[1].waiting = Some(Waiting::Credits);
    model.waiting_words[1] = Some("no credits".to_owned());
    let l = lay(&f, SIZE);
    let mut state = SidebarState::default();
    let out = run(&f, &mut state, &model, at(10.0, 10.0), SIZE);
    let fill = quads_in(&out.ui, color_array(f.look.colors.bar_fill));
    let waiting = quads_in(&out.ui, color_array(f.look.colors.bar_waiting));
    assert_eq!(fill.len(), 1);
    assert_eq!(waiting.len(), 1);
    // 94 px wide tiles: 40 % = 38 px (37.6 rounded), 75 % = 71 px (70.5 rounded).
    assert_eq!(fill[0].w, (l.tile.0 * 0.4).round());
    assert_eq!(waiting[0].w, (l.tile.0 * 0.75).round());
    // Each at the bottom edge of its own tile.
    assert_eq!(fill[0].bottom(), l.slot(0).bottom());
    assert_eq!(waiting[0].bottom(), l.slot(1).bottom());
    assert_eq!(fill[0].x, l.slot(0).x);
    assert_eq!(waiting[0].x, l.slot(1).x);
    // Items without progress have no bar.
    let backs = quads_in(&out.ui, color_array(f.look.colors.bar_back));
    assert_eq!(backs.len(), 2);
}

#[test]
fn the_chosen_structure_gets_a_ring_and_the_others_do_not() {
    let f = fixture();
    let sit = f.situation(Some(1000));
    let mut model = f.model(&sit, Tab::Structures);
    let ring = color_array(f.look.colors.tile_chosen);
    let mut state = SidebarState::default();
    let plain = run(&f, &mut state, &model, at(10.0, 10.0), SIZE);
    assert!(quads_in(&plain.ui, ring).is_empty());
    model.chosen = Some(model.items[2].kind);
    let chosen = run(&f, &mut state, &model, at(10.0, 10.0), SIZE);
    let sides = quads_in(&chosen.ui, ring);
    assert_eq!(sides.len(), 4);
    let slot = lay(&f, SIZE).slot(2);
    assert!(
        sides.iter().all(|r| slot.contains(r.x, r.y)),
        "{sides:?} in {slot:?}"
    );
}

#[test]
fn the_credits_the_power_bar_and_the_tabs_are_drawn() {
    let f = fixture();
    let sit = f.situation(Some(1234));
    let model = f.model(&sit, Tab::Structures);
    let mut state = SidebarState::default();
    let l = lay(&f, SIZE);
    let out = run(&f, &mut state, &model, at(10.0, 10.0), SIZE);
    // The active tab is lit, the others not.
    assert_eq!(
        quads_in(&out.ui, color_array(f.look.colors.tab_active)),
        [l.tabs[0]]
    );
    assert_eq!(quads_in(&out.ui, color_array(f.look.colors.tab)).len(), 3);
    // No power rules: the neutral frame (four strokes), no fill.
    assert_eq!(
        quads_in(&out.ui, color_array(f.look.colors.power_neutral)).len(),
        4
    );
    assert!(quads_in(&out.ui, color_array(f.look.colors.power_fill)).is_empty());
    // The money text sits in the money block; the glyphs come from the same atlas as every text.
    let text_quads_in_money = out
        .ui
        .draw_list()
        .vertices
        .chunks(4)
        .filter(|q| {
            q.len() == 4
                && q[0].pos[0] >= l.money.x
                && q[2].pos[0] <= l.money.right()
                && q[0].pos[1] >= l.money.y
                && q[2].pos[1] <= l.money.bottom()
                && q[0].color == [240, 200, 70, 255]
        })
        .count();
    assert_eq!(text_quads_in_money, "Credits 1234".replace(' ', "").len());
    // A power reading fills the bar in proportion, in the low colour when more is used than made.
    let mut m = model.clone();
    m.power = Some(PowerReading {
        made: 200,
        used: 50,
    });
    let out = run(&f, &mut state, &m, at(10.0, 10.0), SIZE);
    let fill = quads_in(&out.ui, color_array(f.look.colors.power_fill));
    assert_eq!(fill.len(), 1);
    assert_eq!(fill[0].w, (l.power.w * 0.25).round());
    // (The low colour is also the colour of an unaffordable price, so count against the frame
    // before.)
    let low = |ui: &Ui| quads_in(ui, color_array(f.look.colors.power_low)).len();
    let before = low(&out.ui);
    m.power = Some(PowerReading { made: 20, used: 50 });
    let out = run(&f, &mut state, &m, at(10.0, 10.0), SIZE);
    assert_eq!(low(&out.ui), before + 1);
    assert!(quads_in(&out.ui, color_array(f.look.colors.power_fill)).is_empty());
}

#[test]
fn an_empty_tab_says_what_it_needs() {
    let f = fixture();
    let model = f.model(&Situation::default(), Tab::Vessels);
    assert!(model.items.is_empty());
    let mut state = SidebarState {
        tab: Tab::Vessels,
        scroll: 0,
    };
    let empty = run(&f, &mut state, &model, at(10.0, 10.0), SIZE);
    let mut with_items = SidebarState::default();
    let busy = run(
        &f,
        &mut with_items,
        &f.model(&f.situation(Some(1)), Tab::Structures),
        at(10.0, 10.0),
        SIZE,
    );
    // A hint (a few text quads) and no tiles at all, against many quads with items.
    assert!(empty.ui.draw_list().quad_count() < busy.ui.draw_list().quad_count());
    assert!(quads_in(&empty.ui, color_array(f.look.colors.tile_back)).is_empty());
}

#[test]
fn the_sidebar_survives_every_size_and_scale() {
    let f = fixture();
    let sit = f.situation(Some(1000));
    let model = f.model(&sit, Tab::Structures);
    for size in [
        (1280, 720),
        (1920, 1080),
        (800, 600),
        (300, 200),
        (100, 100),
        (0, 0),
    ] {
        for dpi in [0.5, 1.0, 2.0] {
            let mut ui = Ui::new(UiConfig::default());
            let input = press(at(size.0 as f32 - 20.0, 40.0), MouseButton::Left);
            ui.begin_frame(input, size, dpi);
            let mut state = SidebarState::default();
            let _ = frame(&mut ui, &input, &mut state, &model, &f.setup());
            ui.end_frame();
        }
    }
}
