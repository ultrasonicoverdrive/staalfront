//! Selection (WP3.3): which units the player has picked, and the gestures
//! that pick them. Rules in `docs/rules/selection.md`.
//!
//! Selection is **presentation state**, not sim state (PLAN.md section 1.2 and
//! quality rule 1): it lives here in `sf_game`, it is never hashed, never
//! saved in the world and never changes it. All it ever produces is the list
//! of ids that [`crate::commands`] puts into a `Command::Order`.
//!
//! Everything in this module is a pure API with no window, no GPU and no
//! clock: the caller passes the time in milliseconds and what the picture
//! says is under the pointer. The runner feeds it:
//!
//! * [`Pointer`] turns the left button's press, move and release into a
//!   [`Gesture`]: a click (maybe the second of a double-click) or a drag
//!   rectangle in screen pixels. Because the rectangle is in screen space it
//!   works at every zoom, on models and on icons alike (PLAN.md section 2.3);
//!   the render side answers "which units are under this rectangle" with
//!   `Instancer::pick_rect`, and "which unit is under the cursor" with
//!   `Instancer::pick`.
//! * [`Selection`] applies the gesture to the selected units, with the
//!   shift variant ([`Selection::click`], [`Selection::drag`],
//!   [`Selection::select_same_kind`]), the control groups
//!   ([`Selection::assign_group`], [`Selection::recall_group`]) and the
//!   cleanup of the dead ([`Selection::forget_the_dead`], by the generation
//!   check of [`World::is_alive`]).
//!
//! Ownership decides what a unit may do: only the local player's units can
//! be in [`Selection::ids`] and so only they get orders. A unit of anyone
//! else (an enemy, a neutral object) can be **inspected**: it is
//! [`Selection::inspected`], it gets a ring and a health bar, and it never
//! receives an order.

use sf_render::ScreenRect;
use sf_sim::{EntityId, Kind, PlayerId, UnitClass, World};

/// Number of control groups, on the keys 1 to 9.
pub const GROUPS: usize = 9;

/// A press that moves this far (physical pixels) before it is released is a
/// drag, not a click.
pub const DRAG_MIN_PX: f32 = 6.0;

/// Two clicks within this many milliseconds of each other are a double-click
/// (when they are close enough, [`DOUBLE_CLICK_PX`]).
pub const DOUBLE_CLICK_MS: u64 = 400;

/// Two clicks within this many physical pixels of each other are a
/// double-click (when they are quick enough, [`DOUBLE_CLICK_MS`]).
pub const DOUBLE_CLICK_PX: f32 = 8.0;

/// Two taps of the same group key within this many milliseconds centre the
/// camera on the group.
pub const DOUBLE_TAP_MS: u64 = 500;

/// What a selection needs to know about an entity: only things the sim
/// states. An entity that did not come from the unit table (the placeholder
/// wanderers) has none and cannot be selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    /// Who owns it.
    pub owner: PlayerId,
    /// Its role: double-click selects units of the same kind.
    pub kind: Kind,
    /// Whether it is a unit, a structure or an object.
    pub class: UnitClass,
}

/// The [`Facts`] of a live entity; `None` for a stale id (dead, or its slot
/// holds another entity: the generation check) and for an entity without a
/// unit-table entry.
pub fn facts(world: &World, id: EntityId) -> Option<Facts> {
    let stats = world.stats_of(id)?;
    let view = world.get(id)?;
    Some(Facts {
        owner: view.owner,
        kind: stats.kind,
        class: stats.class(),
    })
}

/// What recalling a control group did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Recall {
    /// The group has no living member: the selection is as it was.
    Empty,
    /// The group is now the selection.
    Selected,
    /// A double tap: the group is the selection and the camera should look
    /// at its middle (in cells).
    Centre([f32; 2]),
}

/// The selected units, the inspected one and the control groups.
#[derive(Clone, Debug)]
pub struct Selection {
    local: PlayerId,
    /// The local player's selected entities, ascending by id (the order is
    /// the id's, so a selection built in two ways compares equal).
    own: Vec<EntityId>,
    /// One entity of someone else, picked to look at; never ordered.
    inspected: Option<EntityId>,
    groups: [Vec<EntityId>; GROUPS],
    /// The group key tapped last and when, for the double tap.
    last_tap: Option<(usize, u64)>,
}

impl Selection {
    /// An empty selection for the player at this keyboard.
    pub fn new(local: PlayerId) -> Self {
        Self {
            local,
            own: Vec::new(),
            inspected: None,
            groups: Default::default(),
            last_tap: None,
        }
    }

    /// The selected own entities, ascending by id. Never an enemy or a
    /// neutral one.
    pub fn ids(&self) -> &[EntityId] {
        &self.own
    }

    /// How many own entities are selected.
    pub fn len(&self) -> usize {
        self.own.len()
    }

    /// True if no own entity is selected (an inspected one does not count).
    pub fn is_empty(&self) -> bool {
        self.own.is_empty()
    }

    /// The entity of someone else that is being looked at, if any.
    pub fn inspected(&self) -> Option<EntityId> {
        self.inspected
    }

    /// Whether `id` is selected or inspected: what the feedback draws.
    pub fn is_marked(&self, id: EntityId) -> bool {
        self.own.binary_search(&id).is_ok() || self.inspected == Some(id)
    }

    /// The members of control group `n` (`0` is the key 1).
    pub fn group(&self, n: usize) -> &[EntityId] {
        self.groups.get(n).map_or(&[], Vec::as_slice)
    }

    /// The selected units that can take an order: the mobile ones. A
    /// selected structure is not in it, and neither is anything inspected.
    /// This is the only list orders are made from.
    pub fn orderable(&self, world: &World) -> Vec<EntityId> {
        self.own
            .iter()
            .copied()
            .filter(|&id| facts(world, id).is_some_and(|f| f.class == UnitClass::Mobile))
            .collect()
    }

    /// Empties the selection (the groups stay).
    pub fn clear(&mut self) {
        self.own.clear();
        self.inspected = None;
    }

    fn is_own(&self, f: &Facts) -> bool {
        f.owner == self.local
    }

    /// Replaces the selection with `ids`, or with `ids` added to it.
    fn set_own(&mut self, ids: impl IntoIterator<Item = EntityId>, additive: bool) {
        self.inspected = None;
        if !additive {
            self.own.clear();
        }
        self.own.extend(ids);
        self.own.sort_unstable();
        self.own.dedup();
    }

    /// A single click on `hit`, the unit the picture says is under the
    /// cursor (`None`: bare ground).
    ///
    /// * An own unit or structure becomes the selection; with `additive`
    ///   (shift) it is toggled in the selection instead.
    /// * A unit of someone else is inspected, which replaces the selection;
    ///   with `additive` it is ignored (an enemy cannot join a selection).
    /// * Bare ground (or anything that cannot be selected) empties the
    ///   selection; with `additive` it changes nothing, so a slip of the
    ///   finger does not lose it.
    pub fn click(&mut self, world: &World, hit: Option<EntityId>, additive: bool) {
        let Some((id, f)) = hit.and_then(|id| facts(world, id).map(|f| (id, f))) else {
            if !additive {
                self.clear();
            }
            return;
        };
        if self.is_own(&f) {
            if additive && self.inspected.is_none() && self.own.binary_search(&id).is_ok() {
                self.own.retain(|&x| x != id);
            } else {
                self.set_own([id], additive);
            }
        } else if !additive {
            self.own.clear();
            self.inspected = Some(id);
        }
    }

    /// A double-click on `hit`: every own **unit** of the same kind among
    /// `in_view` (the entities whose picture is on the screen now, from
    /// `Instancer::pick_rect` over the whole viewport) becomes the
    /// selection, or is added to it with `additive`. A structure, a unit of
    /// someone else or bare ground behaves as a single click: structures
    /// are only ever selected one at a time.
    pub fn select_same_kind(
        &mut self,
        world: &World,
        hit: Option<EntityId>,
        in_view: &[EntityId],
        additive: bool,
    ) {
        let Some((id, f)) = hit.and_then(|id| facts(world, id).map(|f| (id, f))) else {
            return self.click(world, hit, additive);
        };
        if !self.is_own(&f) || f.class != UnitClass::Mobile {
            return self.click(world, hit, additive);
        }
        let same = in_view.iter().copied().filter(|&other| {
            facts(world, other).is_some_and(|g| {
                self.is_own(&g) && g.class == UnitClass::Mobile && g.kind == f.kind
            })
        });
        let same: Vec<EntityId> = same.chain([id]).collect();
        self.set_own(same, additive);
    }

    /// A drag rectangle: `in_rect` are the entities whose picture touches
    /// it. The own **units** among them become the selection, or are added
    /// to it with `additive`. Structures, and anything of someone else, are
    /// never taken by a drag. A drag that finds nothing empties the
    /// selection, or with `additive` changes nothing.
    pub fn drag(&mut self, world: &World, in_rect: &[EntityId], additive: bool) {
        let units: Vec<EntityId> = in_rect
            .iter()
            .copied()
            .filter(|&id| {
                facts(world, id).is_some_and(|f| self.is_own(&f) && f.class == UnitClass::Mobile)
            })
            .collect();
        if units.is_empty() {
            if !additive {
                self.clear();
            }
            return;
        }
        self.set_own(units, additive);
    }

    /// Control + number: the selection becomes group `n` (`0` is the key 1).
    /// A unit is in one group at a time, so it leaves the groups it was in.
    /// With nothing selected the group is emptied.
    pub fn assign_group(&mut self, n: usize) {
        if n >= GROUPS {
            return;
        }
        for (i, group) in self.groups.iter_mut().enumerate() {
            if i != n {
                group.retain(|id| self.own.binary_search(id).is_err());
            }
        }
        self.groups[n].clone_from(&self.own);
        self.last_tap = None;
    }

    /// The number alone: the living members of group `n` become the
    /// selection. `now_ms` is the time of the tap; a second tap of the same
    /// number within [`DOUBLE_TAP_MS`] answers [`Recall::Centre`] with the
    /// middle of the group, for the camera. A group with no living member
    /// changes nothing.
    pub fn recall_group(&mut self, world: &World, n: usize, now_ms: u64) -> Recall {
        if n >= GROUPS {
            return Recall::Empty;
        }
        self.groups[n].retain(|&id| world.is_alive(id));
        if self.groups[n].is_empty() {
            self.last_tap = None;
            return Recall::Empty;
        }
        let members = self.groups[n].clone();
        let double = self
            .last_tap
            .is_some_and(|(k, t)| k == n && now_ms.saturating_sub(t) <= DOUBLE_TAP_MS);
        self.set_own(members.iter().copied(), false);
        self.last_tap = (!double).then_some((n, now_ms));
        if double && let Some(middle) = middle_of(world, &members) {
            return Recall::Centre(middle);
        }
        Recall::Selected
    }

    /// Drops what is gone: every dead entity (and every id whose slot now
    /// holds another entity, by the generation check of
    /// [`World::is_alive`]) leaves the selection, the inspected unit and the
    /// groups. Call it once a frame.
    pub fn forget_the_dead(&mut self, world: &World) {
        self.own.retain(|&id| world.is_alive(id));
        if self.inspected.is_some_and(|id| !world.is_alive(id)) {
            self.inspected = None;
        }
        for group in &mut self.groups {
            group.retain(|&id| world.is_alive(id));
        }
    }
}

/// The middle of the entities' positions in cells; `None` if none is alive.
fn middle_of(world: &World, ids: &[EntityId]) -> Option<[f32; 2]> {
    let (mut sum, mut n) = ([0.0_f32; 2], 0.0_f32);
    for v in ids.iter().filter_map(|&id| world.get(id)) {
        let [x, y] = sf_math::render_bridge::vec2_to_f32(v.pos);
        sum = [sum[0] + x, sum[1] + y];
        n += 1.0;
    }
    (n > 0.0).then(|| [sum[0] / n, sum[1] / n])
}

/// What a press and release of the select button came to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// A click at the release position. `double` is true for the second
    /// click of a double-click (a third quick click starts over).
    Click { double: bool },
    /// A drag: the rectangle between the press and the release.
    Drag(ScreenRect),
}

/// The select button's gesture recogniser: press, move, release in; click,
/// double-click or drag out. No clock of its own: the caller passes the time.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pointer {
    down: Option<Down>,
    last_click: Option<([f32; 2], u64)>,
}

#[derive(Clone, Copy, Debug)]
struct Down {
    start: [f32; 2],
    now: [f32; 2],
    dragging: bool,
}

impl Pointer {
    /// The button went down at `at` (physical pixels).
    pub fn press(&mut self, at: [f32; 2]) {
        self.down = Some(Down {
            start: at,
            now: at,
            dragging: false,
        });
    }

    /// The cursor is at `at`. Once it has been [`DRAG_MIN_PX`] away from the
    /// press the gesture is a drag, even if it comes back.
    pub fn moved(&mut self, at: [f32; 2]) {
        if let Some(d) = &mut self.down {
            d.now = at;
            d.dragging |= distance(d.start, at) >= DRAG_MIN_PX;
        }
    }

    /// Whether a press is being held.
    pub fn is_down(&self) -> bool {
        self.down.is_some()
    }

    /// The rectangle to draw while the button is held and the cursor has
    /// moved far enough for a drag.
    pub fn drag_rect(&self) -> Option<ScreenRect> {
        self.down
            .filter(|d| d.dragging)
            .map(|d| ScreenRect::from_corners(d.start, d.now))
    }

    /// The button went up at `at` at time `now_ms`. `None` if no press was
    /// being tracked (it went down on the UI, or was [`cancelled`](Self::cancel)).
    pub fn release(&mut self, at: [f32; 2], now_ms: u64) -> Option<Gesture> {
        self.moved(at);
        let d = self.down.take()?;
        if d.dragging {
            self.last_click = None;
            return Some(Gesture::Drag(ScreenRect::from_corners(d.start, at)));
        }
        let double = self.last_click.is_some_and(|(p, t)| {
            now_ms.saturating_sub(t) <= DOUBLE_CLICK_MS && distance(p, at) <= DOUBLE_CLICK_PX
        });
        self.last_click = (!double).then_some((at, now_ms));
        Some(Gesture::Click { double })
    }

    /// Forgets the press in progress (the window lost focus, the press was
    /// not ours).
    pub fn cancel(&mut self) {
        self.down = None;
    }
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

#[cfg(test)]
mod tests;
