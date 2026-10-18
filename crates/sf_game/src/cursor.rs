//! The cursor context (WP3.5): which cursor the mouse shows, and so which
//! order a right-click gives. Rules and the decision table in
//! `docs/rules/cursor.md`.
//!
//! **One source of truth.** [`cursor_context`] is a pure function of what is
//! selected, what is under the mouse, the held modifiers and the world as the
//! sim thread last published it ([`SimSnapshot`], WP1.15), whose movement costs
//! (`SimSnapshot::passable`, the world's `passable` of WP4.1) it asks which
//! cells a class may enter: the same table the units move by. The
//! picture of the cursor is chosen from its answer, and so is the order of a
//! right-click: [`order_for`] turns the answer into an `Order`, and
//! [`crate::commands::right_click`] is the only thing that schedules it. A
//! cursor that says Attack and a click that moves cannot happen, because
//! nothing else decides.
//!
//! Like [`crate::selection`], this is presentation: it reads the snapshot and
//! never changes the world, and no window, GPU or clock is needed to test it. The
//! picking (which entity is under the mouse, by model close up and by icon
//! zoomed out, PLAN.md section 2.3) happens in `sf_render`; its answer is
//! handed in as [`Hover`], so the context is the same at every zoom.
//!
//! Drawing the cursor is [`CursorArt`] (the pictures of `assets/ui/cursors`,
//! drawn by `sf_ui` over everything else) and the runner (`runner.rs`), which
//! hides the operating system's cursor while the mouse is over the game view.

use std::path::Path;

use sf_engine::{ActionState, Edges};
use sf_math::Vec2;
use sf_sim::{Cell, EntityId, GuardTarget, Order};
use sf_ui::CursorSet;

use crate::sandbox::NEUTRAL;
use crate::selection::{Selection, facts};
use crate::sim_snapshot::SimSnapshot;

/// What the mouse looks like. The first eight are what [`cursor_context`] can
/// answer today; the scroll cursors belong to the screen edges; the last eight
/// are **placeholders**: their pictures exist so that later work packages
/// (capture, repair, enter: phase 6 to 10) only have to teach
/// [`cursor_context`] when to answer them. Until then nothing returns them and
/// [`order_for`] gives no order for them (`placeholders_never_order` tests it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CursorKind {
    /// The pointer: a click selects.
    Select,
    /// Over an enemy or neutral thing with nothing of ours to order: a click
    /// looks at it (WP3.3).
    Inspect,
    /// Over open ground: a right-click moves the selection there.
    Move,
    /// The same with the queue modifier: the move goes behind the orders.
    Queue,
    /// Over an enemy: a right-click attacks it.
    Attack,
    /// Over the ground with the attack-move modifier: go there, fighting.
    AttackMove,
    /// Over one of ours: a right-click guards it.
    Guard,
    /// Over ground no selected unit can enter: a right-click does nothing.
    NoGo,
    ScrollN,
    ScrollNe,
    ScrollE,
    ScrollSe,
    ScrollS,
    ScrollSw,
    ScrollW,
    ScrollNw,
    /// Placeholder (engineers capturing, phase 6 on).
    Capture,
    /// Placeholder (repairing).
    Repair,
    /// Placeholder (entering a structure or a transport).
    Enter,
    /// Placeholder (deploying).
    Deploy,
    /// Placeholder (harvesting).
    Harvest,
    /// Placeholder (infiltrating).
    Infiltrate,
    /// Placeholder (selling).
    Sell,
    /// Placeholder (placing a structure).
    Place,
}

impl CursorKind {
    /// Every cursor, in the order of the files (`tools/sf_icon_gen`).
    pub const ALL: [CursorKind; 24] = [
        CursorKind::Select,
        CursorKind::Inspect,
        CursorKind::Move,
        CursorKind::Queue,
        CursorKind::Attack,
        CursorKind::AttackMove,
        CursorKind::Guard,
        CursorKind::NoGo,
        CursorKind::ScrollN,
        CursorKind::ScrollNe,
        CursorKind::ScrollE,
        CursorKind::ScrollSe,
        CursorKind::ScrollS,
        CursorKind::ScrollSw,
        CursorKind::ScrollW,
        CursorKind::ScrollNw,
        CursorKind::Capture,
        CursorKind::Repair,
        CursorKind::Enter,
        CursorKind::Deploy,
        CursorKind::Harvest,
        CursorKind::Infiltrate,
        CursorKind::Sell,
        CursorKind::Place,
    ];

    /// The name in `cursors.ron` and the file stem of the picture.
    pub const fn name(self) -> &'static str {
        match self {
            CursorKind::Select => "select",
            CursorKind::Inspect => "inspect",
            CursorKind::Move => "move",
            CursorKind::Queue => "queue",
            CursorKind::Attack => "attack",
            CursorKind::AttackMove => "attack_move",
            CursorKind::Guard => "guard",
            CursorKind::NoGo => "no_go",
            CursorKind::ScrollN => "scroll_n",
            CursorKind::ScrollNe => "scroll_ne",
            CursorKind::ScrollE => "scroll_e",
            CursorKind::ScrollSe => "scroll_se",
            CursorKind::ScrollS => "scroll_s",
            CursorKind::ScrollSw => "scroll_sw",
            CursorKind::ScrollW => "scroll_w",
            CursorKind::ScrollNw => "scroll_nw",
            CursorKind::Capture => "capture",
            CursorKind::Repair => "repair",
            CursorKind::Enter => "enter",
            CursorKind::Deploy => "deploy",
            CursorKind::Harvest => "harvest",
            CursorKind::Infiltrate => "infiltrate",
            CursorKind::Sell => "sell",
            CursorKind::Place => "place",
        }
    }

    /// The position in [`CursorKind::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// One of the eight directional scroll cursors.
    pub const fn is_scroll(self) -> bool {
        matches!(
            self,
            CursorKind::ScrollN
                | CursorKind::ScrollNe
                | CursorKind::ScrollE
                | CursorKind::ScrollSe
                | CursorKind::ScrollS
                | CursorKind::ScrollSw
                | CursorKind::ScrollW
                | CursorKind::ScrollNw
        )
    }

    /// A picture whose meaning a later work package gives; it never gives an
    /// order today.
    pub const fn is_placeholder(self) -> bool {
        matches!(
            self,
            CursorKind::Capture
                | CursorKind::Repair
                | CursorKind::Enter
                | CursorKind::Deploy
                | CursorKind::Harvest
                | CursorKind::Infiltrate
                | CursorKind::Sell
                | CursorKind::Place
        )
    }
}

/// The modifiers held while the mouse moves and clicks: the actions
/// `OrderQueue` and `OrderAttackMove` of `assets/config/input.ron`, never
/// their keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// The order goes behind the unit's orders instead of replacing them.
    pub queue: bool,
    /// A right-click on the ground attack-moves instead of moving.
    pub attack_move: bool,
}

impl Modifiers {
    /// The modifiers of this frame.
    pub fn from_actions(actions: &ActionState) -> Self {
        Self {
            queue: actions.held(sf_engine::Action::OrderQueue),
            attack_move: actions.held(sf_engine::Action::OrderAttackMove),
        }
    }
}

/// What is under the mouse this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hover {
    /// The entity whose picture is under the mouse: the model's box close up,
    /// the icon zoomed out (`Instancer::pick`, the same answer the hover
    /// highlight and a click use). `None` over bare ground, over the UI and
    /// when the mouse is outside the window.
    pub entity: Option<EntityId>,
    /// The ground point under the mouse in cells (`Camera::ground_point`);
    /// `None` over the UI, over the sky and outside the window.
    pub point: Option<Vec2>,
    /// The screen edges the mouse is scrolling.
    pub edges: Edges,
}

impl Hover {
    /// The middle of `cell`, with nothing on it: a convenience for tests.
    pub fn ground(cell: Cell) -> Self {
        Self {
            point: Some(Vec2::new(
                sf_math::Fx::from_int(cell.x as i32) + sf_math::Fx::HALF,
                sf_math::Fx::from_int(cell.y as i32) + sf_math::Fx::HALF,
            )),
            ..Self::default()
        }
    }

    /// `entity` at the middle of `cell`.
    pub fn over(entity: EntityId, cell: Cell) -> Self {
        Self {
            entity: Some(entity),
            ..Self::ground(cell)
        }
    }
}

/// Who an entity is to the player at this keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    Own,
    Neutral,
    Enemy,
}

fn relation(selection: &Selection, owner: sf_sim::PlayerId) -> Relation {
    if owner == selection.local() {
        Relation::Own
    } else if owner.0 == NEUTRAL {
        Relation::Neutral
    } else {
        Relation::Enemy
    }
}

/// The scroll cursor of the screen edges the mouse is at: the corners are the
/// diagonals; opposite edges at once (a window narrower than two bands) cancel.
pub fn scroll_cursor(edges: Edges) -> Option<CursorKind> {
    let horizontal = i8::from(edges.right) - i8::from(edges.left);
    // Screen y grows downwards: the top edge scrolls north.
    let vertical = i8::from(edges.top) - i8::from(edges.bottom);
    Some(match (horizontal, vertical) {
        (0, 1) => CursorKind::ScrollN,
        (1, 1) => CursorKind::ScrollNe,
        (1, 0) => CursorKind::ScrollE,
        (1, -1) => CursorKind::ScrollSe,
        (0, -1) => CursorKind::ScrollS,
        (-1, -1) => CursorKind::ScrollSw,
        (-1, 0) => CursorKind::ScrollW,
        (-1, 1) => CursorKind::ScrollNw,
        _ => return None,
    })
}

/// The cursor for this mouse position, selection and modifiers. Pure: the
/// same inputs give the same answer, nothing is changed and nothing is
/// allocated. The rules, in order (`docs/rules/cursor.md` has the table):
///
/// 1. At a screen edge: the scroll cursor of that edge (it wins over all).
/// 2. No selected unit can take an order (nothing selected, or only
///    structures): over an enemy or neutral thing **Inspect**, else
///    **Select**.
/// 3. Over an enemy and at least one selected unit has a weapon: **Attack**.
/// 4. Over one of ours that is not the only selected unit: **Guard**.
/// 5. Over the ground (also under an entity the rules above did not take):
///    no mouse ground point: **Select**; off the map or ground that no
///    selected unit can enter: **NoGo**; the attack-move modifier with an
///    armed unit: **AttackMove**; the queue modifier: **Queue**; else
///    **Move**.
///
/// Orders go to the selected units that can take them
/// ([`Selection::orderable`]), so a selected structure, armed or not, counts
/// for nothing here.
pub fn cursor_context(
    world: &SimSnapshot,
    selection: &Selection,
    hover: &Hover,
    modifiers: Modifiers,
) -> CursorKind {
    if let Some(scroll) = scroll_cursor(hover.edges) {
        return scroll;
    }
    let under = hover
        .entity
        .and_then(|id| facts(world, id).map(|f| (id, relation(selection, f.owner))));

    // What the orders would go to.
    let (mut mobile, mut armed, mut hovered_among_them) = (0_u32, false, false);
    for id in selection.orderable_iter(world) {
        mobile += 1;
        hovered_among_them |= under.is_some_and(|(hovered, _)| hovered == id);
        armed |= world
            .stats_of(id)
            .is_some_and(|s| s.weapon_range > sf_math::Fx::ZERO);
    }
    if mobile == 0 {
        return match under {
            Some((_, Relation::Enemy | Relation::Neutral)) => CursorKind::Inspect,
            _ => CursorKind::Select,
        };
    }

    match under {
        Some((_, Relation::Enemy)) if armed => return CursorKind::Attack,
        // A unit cannot guard itself: the only selected unit under the mouse
        // is ground like any other.
        Some((_, Relation::Own)) if mobile > u32::from(hovered_among_them) => {
            return CursorKind::Guard;
        }
        _ => {}
    }

    let Some(point) = hover.point else {
        return CursorKind::Select;
    };
    let Some(cell) = world.cell_at(point) else {
        return CursorKind::NoGo;
    };
    // A unit without a movement class (none of the orderable ones) could go
    // anywhere on the map; the cell is on the map here.
    let can_go = selection.orderable_iter(world).any(|id| {
        match world.stats_of(id).and_then(|s| s.movement_class) {
            Some(class) => world.passable(class, cell),
            None => true,
        }
    });
    if !can_go {
        CursorKind::NoGo
    } else if modifiers.attack_move && armed {
        CursorKind::AttackMove
    } else if modifiers.queue {
        CursorKind::Queue
    } else {
        CursorKind::Move
    }
}

/// The order a right-click gives when the cursor is `kind`; `None` when the
/// cursor promises nothing (Select, Inspect, NoGo, the scroll cursors and the
/// placeholders). This is the only place a cursor becomes an order.
pub fn order_for(kind: CursorKind, hover: &Hover) -> Option<Order> {
    match kind {
        CursorKind::Move | CursorKind::Queue => hover.point.map(Order::Move),
        CursorKind::AttackMove => hover.point.map(Order::AttackMove),
        CursorKind::Attack => hover.entity.map(Order::Attack),
        CursorKind::Guard => hover.entity.map(|id| Order::Guard(GuardTarget::Entity(id))),
        CursorKind::Select
        | CursorKind::Inspect
        | CursorKind::NoGo
        | CursorKind::ScrollN
        | CursorKind::ScrollNe
        | CursorKind::ScrollE
        | CursorKind::ScrollSe
        | CursorKind::ScrollS
        | CursorKind::ScrollSw
        | CursorKind::ScrollW
        | CursorKind::ScrollNw
        | CursorKind::Capture
        | CursorKind::Repair
        | CursorKind::Enter
        | CursorKind::Deploy
        | CursorKind::Harvest
        | CursorKind::Infiltrate
        | CursorKind::Sell
        | CursorKind::Place => None,
    }
}

/// Whether the order of `kind` goes behind the unit's orders: the Queue
/// cursor is a queued move, and the queue modifier queues an attack, an
/// attack-move or a guard as well.
pub fn is_queued(kind: CursorKind, modifiers: Modifiers) -> bool {
    kind == CursorKind::Queue
        || (modifiers.queue
            && matches!(
                kind,
                CursorKind::Attack | CursorKind::AttackMove | CursorKind::Guard
            ))
}

/// The pictures of the cursors in the UI's atlas, by [`CursorKind`].
pub struct CursorArt {
    set: CursorSet,
    /// The sprite of each kind, by [`CursorKind::index`].
    sprite: [usize; CursorKind::ALL.len()],
}

impl CursorArt {
    /// Matches every cursor kind with its picture in `set`; an error naming
    /// the kinds the set lacks.
    pub fn new(set: CursorSet) -> Result<Self, String> {
        let mut sprite = [0; CursorKind::ALL.len()];
        let mut missing = Vec::new();
        for kind in CursorKind::ALL {
            match set.index_of(kind.name()) {
                Some(i) => sprite[kind.index()] = i,
                None => missing.push(kind.name()),
            }
        }
        if missing.is_empty() {
            Ok(Self { set, sprite })
        } else {
            Err(format!("cursors.ron has no {}", missing.join(", ")))
        }
    }

    /// Loads `dir` (`assets/ui/cursors`) into the HUD's atlas.
    pub fn load(hud: &mut crate::hud::Hud, dir: &Path) -> Result<Self, String> {
        let set = hud.load_cursors(dir).map_err(|e| e.to_string())?;
        Self::new(set)
    }

    /// The picture set, for `Ui::draw_cursor`.
    pub fn set(&self) -> &CursorSet {
        &self.set
    }

    /// The sprite index of `kind`.
    pub fn sprite_of(&self, kind: CursorKind) -> usize {
        self.sprite[kind.index()]
    }
}

#[cfg(test)]
mod tests;
