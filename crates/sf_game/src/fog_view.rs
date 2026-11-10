//! What the picture does with shroud and fog (WP8.3, WP8.4, `docs/rules/fog_look.md`): the ghosts of
//! structures last seen, the choice of what to draw, and the bookkeeping that uploads the fog only when
//! it changed. Presentation only; it reads the snapshot's picture of what the local player sees
//! ([`SimSnapshot::view_shown`], [`SimSnapshot::fog_words`]) and decides nothing in the simulation.
//!
//! **Only with vision rules.** Every function here does nothing in a world whose snapshot has no
//! [`SimSnapshot::fog_version`]: nothing is hidden, there are no ghosts, no fog is uploaded.
//!
//! # Ghosts
//!
//! A *ghost* is the last view the local player had of a structure that is not its own: a structure of
//! the other side or a neutral one. [`GhostBook::update`] runs once per new snapshot:
//!
//! * a structure that is seen now is written (or rewritten) in the book, with what it looked like (its
//!   shape, which carries the damage stage and the stage of construction);
//! * a record whose structure is **not** seen is drawn, in the ghost colour, as long as no cell of its
//!   footprint is visible;
//! * a record whose structure is not seen **and** has a visible cell is removed: the player looks at
//!   the place and nothing is there any more (destroyed, sold, or never finished). A structure that is
//!   alive keeps its record while it is seen, and is drawn for real then.
//!
//! Units leave no ghosts. A new scene (a restart, a main-menu round trip) clears the book
//! ([`GhostBook::clear`]); so does a snapshot of an earlier tick than the last one.

use std::collections::{BTreeMap, BTreeSet};

use sf_data::damage::Stage;
use sf_data::fog_look::FogLook;
use sf_math::{Facing, Vec2};
use sf_render::{Motion, UnitId};
use sf_sim::{Cell, EntityId, EntityView, Kind, PlayerId};

use crate::looks::{GHOST_COLOUR, Looks};
use crate::sim_snapshot::SimSnapshot;

/// The shipped look (`assets/config/fog_look.ron`), read once.
pub fn shipped_look() -> &'static FogLook {
    static LOOK: std::sync::OnceLock<FogLook> = std::sync::OnceLock::new();
    LOOK.get_or_init(FogLook::embedded)
}

/// The terrain shader's settings for `look`, in linear light.
pub fn shading_of(look: &FogLook) -> sf_render::FogShading {
    sf_render::FogShading {
        dim: look.fog_dim,
        tint: sf_render::fog::rgb_to_linear(look.fog_tint),
        shroud: sf_render::fog::rgb_to_linear(look.shroud),
    }
}

/// The minimap's settings for `look`.
pub fn minimap_fog_of(look: &FogLook) -> sf_ui::MinimapFog {
    sf_ui::MinimapFog {
        shroud: [
            look.minimap_shroud.0,
            look.minimap_shroud.1,
            look.minimap_shroud.2,
        ],
        fog_dim: look.minimap_fog_dim,
    }
}

/// The generation the picture gives the id of a ghost (see `looks::WRECK_GENERATION`): the slot keeps
/// it apart from every entity, the value only makes a ghost's id easy to recognise.
const GHOST_GENERATION: u32 = u32::MAX - 1;

/// The last view of a structure of the other side or a neutral one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ghost {
    /// The structure's id when it was seen.
    pub id: EntityId,
    /// What it is.
    pub kind: Kind,
    /// Whose it was.
    pub owner: PlayerId,
    /// The north-west cell of its footprint.
    pub anchor: Cell,
    /// Footprint width in cells.
    pub width: u8,
    /// Footprint height in cells.
    pub height: u8,
    /// Where its middle was, in cells.
    pub pos: Vec2,
    /// The way it faced.
    pub facing: Facing,
    /// How damaged it looked.
    pub stage: Stage,
    /// The shape the picture drew it with (the model of its stage of damage or construction).
    pub shape: u8,
}

impl Ghost {
    /// Whether any cell of the footprint is visible to the local player now.
    fn any_cell_visible(&self, snap: &SimSnapshot) -> bool {
        (0..u32::from(self.height)).any(|dy| {
            (0..u32::from(self.width))
                .any(|dx| snap.is_visible(Cell::new(self.anchor.x + dx, self.anchor.y + dy)))
        })
    }
}

/// The ghosts of a game. See the module documentation.
#[derive(Clone, Debug, Default)]
pub struct GhostBook {
    ghosts: BTreeMap<EntityId, Ghost>,
    /// The tick of the last snapshot read; an earlier one means a new game.
    last_tick: u64,
}

impl GhostBook {
    /// An empty book.
    pub fn new() -> GhostBook {
        GhostBook::default()
    }

    /// Forgets everything (a restart, a new scene).
    pub fn clear(&mut self) {
        self.ghosts.clear();
        self.last_tick = 0;
    }

    /// How many ghosts are remembered (seen structures included; see [`GhostBook::drawn`]).
    pub fn len(&self) -> usize {
        self.ghosts.len()
    }

    /// Whether nothing is remembered.
    pub fn is_empty(&self) -> bool {
        self.ghosts.is_empty()
    }

    /// The record of `id`, if there is one.
    pub fn get(&self, id: EntityId) -> Option<&Ghost> {
        self.ghosts.get(&id)
    }

    /// Reads a new snapshot. `look` says which shape a view is drawn with and how damaged it is.
    pub fn update_with(&mut self, snap: &SimSnapshot, look: &dyn Fn(&EntityView) -> (u8, Stage)) {
        if snap.fog_version().is_none() {
            self.clear();
            return;
        }
        if snap.tick().0 < self.last_tick {
            self.ghosts.clear();
        }
        self.last_tick = snap.tick().0;
        let local = snap.local_player();
        let mut alive_shown: BTreeSet<EntityId> = BTreeSet::new();
        for (i, v) in snap.views().iter().enumerate() {
            let Some((anchor, width, height)) = snap.footprint_at(i) else {
                continue;
            };
            if v.owner == local || !snap.view_shown(i) {
                continue;
            }
            let (shape, stage) = look(v);
            self.ghosts.insert(
                v.id,
                Ghost {
                    id: v.id,
                    kind: v.kind,
                    owner: v.owner,
                    anchor,
                    width,
                    height,
                    pos: v.pos,
                    facing: v.facing,
                    stage,
                    shape,
                },
            );
            alive_shown.insert(v.id);
        }
        // A record whose structure is not shown and whose place is in view again: nothing is there.
        self.ghosts
            .retain(|id, g| alive_shown.contains(id) || !g.any_cell_visible(snap));
    }

    /// [`GhostBook::update_with`] with the shapes `looks` gives (the same as the real structures are
    /// drawn with, `runner::motion_of`).
    pub fn update(&mut self, snap: &SimSnapshot, looks: &Looks) {
        self.update_with(snap, &|v| {
            let building = snap.construction_of(v.id).map(|c| c.permille());
            let stage = looks.damage_stage(snap, v);
            (looks.shape_in_state(v, building, stage), stage)
        });
    }

    /// The ghosts to draw now: the records whose structure is not shown. A structure that is seen is
    /// drawn as itself, and its record only waits.
    pub fn drawn<'a>(&'a self, snap: &'a SimSnapshot) -> impl Iterator<Item = &'a Ghost> + 'a {
        // `seen` is false for a structure that is hidden and for one that is gone (a stale id).
        self.ghosts
            .values()
            .filter(move |g| snap.fog_version().is_some() && !snap.seen(g.id))
    }

    /// The ghosts as models for the instancer: in the ghost colour, numbered from `first_slot` on (a
    /// slot no entity or wreck has, so a pick of a ghost names nothing).
    pub fn motions(&self, snap: &SimSnapshot, first_slot: u32) -> Vec<Motion> {
        self.drawn(snap)
            .enumerate()
            .map(|(n, g)| {
                Motion::from_fixed(g.shape, GHOST_COLOUR, g.pos, g.pos, g.facing, g.facing)
                    .with_id(UnitId::from_parts(first_slot + n as u32, GHOST_GENERATION))
                    // A ghost is a structure last seen: the structure scale, like the original.
                    .with_structure(true)
            })
            .collect()
    }
}

/// One dot of the minimap.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dot {
    /// A unit or structure of a player.
    Owner(u8),
    /// The ghost of a structure.
    Ghost,
}

/// The dots the minimap shows (WP8.4): every view that is shown (the local player's own, and the
/// others that are seen), then the ghosts. `pos` is in cells.
pub fn minimap_dots<'a>(
    snap: &'a SimSnapshot,
    ghosts: &'a GhostBook,
) -> impl Iterator<Item = (Vec2, Dot)> + 'a {
    let views = snap
        .views()
        .iter()
        .enumerate()
        .filter(|&(i, _)| snap.view_shown(i))
        .map(|(_, v)| (v.pos, Dot::Owner(v.owner.0)));
    let ghost_dots = ghosts.drawn(snap).map(|g| (g.pos, Dot::Ghost));
    views.chain(ghost_dots)
}

/// What the renderer has been given of the fog, so that it is uploaded only when `fog_version`
/// changes ("updated only when `fog_version` changes", WP8.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FogApplied {
    /// The version last given to the terrain, if the terrain shows fog.
    pub terrain: Option<u64>,
    /// The version last given to the minimap, if it shows fog.
    pub minimap: Option<u64>,
}

/// What to do with a surface that shows version `applied`, given the snapshot's `current`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogStep {
    /// Nothing: it shows what the snapshot says.
    Keep,
    /// Build and upload the picture of this version.
    Upload(u64),
    /// Take the fog off (the world has no vision rules).
    Clear,
}

/// The step for one surface.
pub fn fog_step(applied: Option<u64>, current: Option<u64>) -> FogStep {
    match (applied, current) {
        (a, Some(c)) if a == Some(c) => FogStep::Keep,
        (_, Some(c)) => FogStep::Upload(c),
        (Some(_), None) => FogStep::Clear,
        (None, None) => FogStep::Keep,
    }
}

/// Gives the terrain the fog of `snap`, or takes it off, when `applied` (the version it shows) is not
/// the snapshot's: the one place the terrain's fog is set (the runner and the screenshot tests call it).
/// Returns what it did. A terrain of another size than the world's gets no fog (and the version is
/// marked as applied so that the texels are not rebuilt every frame).
pub fn sync_terrain(
    renderer: &mut sf_render::Renderer,
    snap: &SimSnapshot,
    applied: &mut Option<u64>,
) -> FogStep {
    let step = fog_step(*applied, snap.fog_version());
    match step {
        FogStep::Keep => {}
        FogStep::Upload(v) => {
            let look = shipped_look();
            let (w, h) = snap.map_size();
            if renderer.fog_size() == Some((w, h)) {
                let (visible, explored) = snap.fog_words();
                let texels = sf_render::fog::texels(w, h, visible, explored, look.softness);
                renderer.set_fog((w, h), &texels, shading_of(look));
            } else {
                eprintln!(
                    "sf_game: the fog of the world is {w}x{h} cells, the terrain's is {:?}; no fog drawn",
                    renderer.fog_size()
                );
            }
            *applied = Some(v);
        }
        FogStep::Clear => {
            renderer.clear_fog();
            *applied = None;
        }
    }
    step
}

impl FogApplied {
    /// The surfaces lost what they showed (a new terrain, a new minimap): upload again.
    pub fn forget(&mut self) {
        *self = FogApplied::default();
    }
}

#[cfg(test)]
mod tests;
