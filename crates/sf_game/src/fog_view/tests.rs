//! The presentation of shroud and fog (WP8.3, WP8.4): what is drawn, iconised, picked and put on the
//! minimap; the ghosts; the effects; the bookkeeping of the fog upload. Every test goes through the
//! production functions (`runner::motions_of`, `GhostBook`, `Instancer`, `Minimap::set_fog`,
//! `FxPlayer`) on a real world with vision rules, and asserts the other way round too (a visible
//! thing is there), so a swapped test or a filter that hides everything fails.

use std::collections::BTreeSet;

use sf_data::units::UnitDefs;
use sf_math::{Fx, Vec2};
use sf_render::camera::DVec2;
use sf_render::glam::{Mat4, Vec3};
use sf_render::model::EyeView;
use sf_render::{Blend, Camera, CameraConfig, FrameView, Frustum, Instancer, PickQuery, Style};
use sf_sim::{Cell, Command, EntityId, Kind, PlayerId, World, WorldConfig};

use super::*;
use crate::commands::LOCAL_PLAYER;
use crate::looks::Looks;
use crate::population::Population;
use crate::runner::{motions_of, scene_motions};
use crate::sim_snapshot::SimSnapshot;

const ENEMY: PlayerId = PlayerId(1);
const W: u32 = 64;
const H: u32 = 48;

fn at(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

fn defs() -> UnitDefs {
    UnitDefs::embedded()
}

fn kind(role: &str) -> Kind {
    Kind(defs().kind_of(role).unwrap())
}

fn world(vision: bool) -> World {
    let d = defs();
    let w = World::new(WorldConfig::new(5).with_map_size(W, H))
        .unwrap()
        .with_units(sf_bridge::unit_table(&d).unwrap())
        .unwrap();
    if vision {
        crate::playfield::with_vision(w, &d)
    } else {
        w
    }
}

fn spawn(w: &mut World, owner: PlayerId, role: &str, x: i32, y: i32) {
    let t = w.tick();
    w.schedule(
        t,
        owner,
        Command::SpawnUnit {
            kind: kind(role),
            pos: at(x, y),
            owner,
        },
    )
    .unwrap();
}

fn place(w: &mut World, owner: PlayerId, role: &str, x: u32, y: u32) {
    let t = w.tick();
    w.schedule(
        t,
        owner,
        Command::PlaceStructure {
            kind: kind(role),
            cell: Cell::new(x, y),
            owner,
        },
    )
    .unwrap();
}

fn looks() -> Looks {
    Looks::Crowd(Population::new(None))
}

/// The ids the instancer is given for `snap`.
fn motion_ids(snap: &SimSnapshot) -> BTreeSet<u32> {
    motions_of(&looks(), snap).map(|m| m.id.index()).collect()
}

fn slot_of(w: &World, x: u32, y: u32) -> u32 {
    w.occupant_at(Cell::new(x, y)).unwrap().index()
}

/// The slot of the unit of `owner` whose cell column is `x` (the field's units stand in different columns).
fn slot_at_column(snap: &SimSnapshot, owner: PlayerId, x: i32) -> u32 {
    snap.views()
        .iter()
        .find(|v| v.owner == owner && v.pos.cell().0 == x)
        .expect("a unit in that column")
        .id
        .index()
}

/// The local scout (sight 8) at (10, 10), an enemy tank beside it at (13, 10) that it sees, an enemy
/// tank at (50, 30) that it does not, and a tank of the local player at (30, 40) far from the scout.
fn field() -> (World, SimSnapshot) {
    let mut w = world(true);
    spawn(&mut w, LOCAL_PLAYER, "scout", 10, 10);
    spawn(&mut w, ENEMY, "light_tank", 13, 10);
    spawn(&mut w, ENEMY, "light_tank", 50, 30);
    spawn(&mut w, LOCAL_PLAYER, "light_tank", 30, 40);
    w.step();
    let snap = SimSnapshot::capture(&w);
    (w, snap)
}

// ---- hiding: the list the instancer gets ---------------------------------------------------------

#[test]
fn an_enemy_unit_out_of_sight_is_not_given_to_the_instancer_and_the_others_are() {
    let (_w, snap) = field();
    assert_eq!(snap.views().len(), 4);
    let ids = motion_ids(&snap);
    let scout = slot_at_column(&snap, LOCAL_PLAYER, 10);
    let near = slot_at_column(&snap, ENEMY, 13);
    let far = slot_at_column(&snap, ENEMY, 50);
    let tank = slot_at_column(&snap, LOCAL_PLAYER, 30);
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert!(ids.contains(&scout), "the scout (own, seen)");
    assert!(ids.contains(&near), "the enemy tank in sight");
    assert!(
        !ids.contains(&far),
        "the enemy tank in the shroud is hidden"
    );
    assert!(ids.contains(&tank), "an own tank is always shown");
}

#[test]
fn own_objects_are_shown_even_when_their_own_cell_is_not_in_sight() {
    // A legacy structure never stands, so it sees nothing; it is own, so it is shown anyway, and the
    // enemy's twin in fog is not.
    let mut w = world(true);
    let t = w.tick();
    w.schedule(
        t,
        LOCAL_PLAYER,
        Command::Place {
            kind: Kind(900),
            cell: Cell::new(5, 5),
            owner: LOCAL_PLAYER,
        },
    )
    .unwrap();
    w.schedule(
        t,
        ENEMY,
        Command::Place {
            kind: Kind(901),
            cell: Cell::new(20, 20),
            owner: ENEMY,
        },
    )
    .unwrap();
    w.step();
    let snap = SimSnapshot::capture(&w);
    let (own, theirs) = (slot_of(&w, 5, 5), slot_of(&w, 20, 20));
    let ids = motion_ids(&snap);
    assert!(ids.contains(&own), "own is shown");
    assert!(!ids.contains(&theirs), "the enemy's is hidden");
}

#[test]
fn a_world_without_vision_rules_hides_nothing() {
    let mut w = world(false);
    spawn(&mut w, LOCAL_PLAYER, "scout", 10, 10);
    spawn(&mut w, ENEMY, "light_tank", 50, 30);
    spawn(&mut w, ENEMY, "rifleman", 5, 40);
    w.step();
    let snap = SimSnapshot::capture(&w);
    assert_eq!(snap.fog_version(), None);
    assert_eq!(motion_ids(&snap).len(), 3, "every view is drawn");
    assert!((0..3).all(|i| snap.view_shown(i)));
    assert!(snap.shown_at_metres([200.0, 100.0], ENEMY));
    let mut book = GhostBook::new();
    book.update(&snap, &looks());
    assert!(book.is_empty());
    assert_eq!(book.drawn(&snap).count(), 0);
    assert_eq!(book.motions(&snap, 100).len(), 0);
    assert_eq!(minimap_dots(&snap, &book).count(), 3);
    assert_eq!(fog_step(None, snap.fog_version()), FogStep::Keep);
}

/// The camera matrix and frustum of a view of the whole map.
fn overhead() -> (Mat4, Frustum) {
    let cfg = CameraConfig {
        map_size: DVec2::new(f64::from(W) * 4.0, f64::from(H) * 4.0),
        ..CameraConfig::default()
    };
    let mut cam = Camera::new(cfg, 960, 720);
    cam.set_zoom(1.0);
    cam.set_focus(DVec2::new(f64::from(W) * 2.0, f64::from(H) * 2.0));
    let vp = cam.view_proj();
    (vp, Frustum::from_view_proj(vp))
}

fn instancer() -> Instancer {
    Instancer::new(
        crate::looks::with_wreck_colour(Style::embedded().scaled(3.0)),
        (W as f32 * 4.0, H as f32 * 4.0),
    )
}

#[test]
fn the_icon_list_and_the_model_lists_leave_out_what_is_not_seen() {
    let (_w, snap) = field();
    let (vp, frustum) = overhead();
    let eye = EyeView::from_view_proj(vp, 720.0);
    let mut inst = instancer();
    inst.update_tick(motions_of(&looks(), &snap));
    let flat = |_: f32, _: f32| 0.0;
    let stats = inst.build_frame(&FrameView::new(&frustum, 0.0, eye, Blend::ICONS, 1.0), flat);
    assert_eq!(stats.icons, 3, "scout, the enemy in sight, the own tank");
    assert_eq!(inst.icon_instances().len(), 3);
    let models = inst.build_frame(
        &FrameView::new(&frustum, 0.0, eye, Blend::MODELS, 1.0),
        flat,
    );
    assert_eq!(models.models, 3);
    assert_eq!(models.visible, 3);
}

#[test]
fn picking_finds_a_seen_enemy_and_nothing_where_the_hidden_one_stands() {
    let (_w, snap) = field();
    let (vp, frustum) = overhead();
    let mut inst = instancer();
    inst.update_tick(motions_of(&looks(), &snap));
    inst.build_frame(&FrameView::models_only(&frustum, 0.0), |_, _| 0.0);
    let size = [960.0, 720.0];
    let px = |x: f32, z: f32| {
        sf_render::pick::project_px(&vp, Vec3::new(x * 4.0 + 2.0, 1.0, z * 4.0 + 2.0), size)
            .map(|(p, _)| p)
            .unwrap()
    };
    let near = inst.pick(&PickQuery::new(px(13.0, 10.0), vp, size));
    let near_slot = slot_at_column(&snap, ENEMY, 13);
    assert_eq!(
        near.map(|p| p.id.index()),
        Some(near_slot),
        "the enemy in sight"
    );
    let hidden = inst.pick(&PickQuery::new(px(50.0, 30.0), vp, size));
    assert!(hidden.is_none(), "{hidden:?}");
}

#[test]
fn selection_facts_and_the_inspected_unit_ignore_what_is_not_seen() {
    let (_w, snap) = field();
    let id_in = |x: i32| {
        snap.views()
            .iter()
            .find(|v| v.owner == ENEMY && v.pos.cell().0 == x)
            .expect("an enemy in that column")
            .id
    };
    let (near_enemy, far_enemy) = (id_in(13), id_in(50));
    assert!(crate::selection::facts(&snap, near_enemy).is_some());
    assert!(
        crate::selection::facts(&snap, far_enemy).is_none(),
        "a hidden unit has no facts: no inspect, no attack cursor"
    );
    // A click on the hidden one selects nothing.
    let mut sel = crate::selection::Selection::new(LOCAL_PLAYER);
    sel.click(&snap, Some(far_enemy), false);
    assert!(sel.inspected().is_none());
    sel.click(&snap, Some(near_enemy), false);
    assert_eq!(sel.inspected(), Some(near_enemy));
}

// ---- ghosts ---------------------------------------------------------------------------------------

/// Who sees what: the scout of the local player at (10, 10) sees the enemy refinery anchored at (14, 8)
/// (3 by 4 cells), then walks off to (40, 40).
fn ghost_world() -> World {
    let mut w = world(true);
    spawn(&mut w, LOCAL_PLAYER, "scout", 10, 10);
    place(&mut w, ENEMY, "refinery", 14, 8);
    w.step();
    w
}

fn scout_id(w: &World) -> EntityId {
    w.snapshot()
        .iter()
        .find(|v| v.owner == LOCAL_PLAYER)
        .unwrap()
        .id
}

fn send_scout(w: &mut World, x: i32, y: i32) {
    let id = scout_id(w);
    let t = w.tick();
    w.schedule(
        t,
        LOCAL_PLAYER,
        Command::MoveTo {
            ids: vec![id],
            target: at(x, y),
        },
    )
    .unwrap();
}

fn run(w: &mut World, ticks: u32) {
    for _ in 0..ticks {
        w.step();
    }
}

#[test]
fn a_structure_seen_and_then_left_behind_becomes_a_ghost_and_is_removed_when_seen_gone() {
    let mut w = ghost_world();
    let refinery = w.occupant_at(Cell::new(14, 8)).unwrap();
    let mut book = GhostBook::new();
    let l = looks();

    // Seen: a record exists, nothing is drawn as a ghost (the real thing is drawn).
    let snap = SimSnapshot::capture(&w);
    assert!(snap.seen(refinery));
    book.update(&snap, &l);
    assert_eq!(book.len(), 1);
    assert_eq!(book.drawn(&snap).count(), 0, "it is drawn as itself");
    let g = book.get(refinery).unwrap();
    assert_eq!(g.owner, ENEMY);
    assert_eq!(g.kind, kind("refinery"));
    assert_eq!((g.anchor, g.width, g.height), (Cell::new(14, 8), 3, 4));

    // The scout leaves: the cells go to fog, the real refinery is hidden, the ghost is drawn.
    send_scout(&mut w, 40, 40);
    run(&mut w, 600);
    let snap = SimSnapshot::capture(&w);
    assert_eq!(snap.visibility(Cell::new(14, 8)), sf_sim::Visibility::Fog);
    assert!(!snap.seen(refinery));
    book.update(&snap, &l);
    let drawn: Vec<_> = book.drawn(&snap).map(|g| g.id).collect();
    assert_eq!(drawn, vec![refinery]);
    // As a model for the instancer, in the ghost colour, in a slot no entity has.
    let m = scene_motions(&l, &snap, &book).collect::<Vec<_>>();
    assert_eq!(
        m.len(),
        snap.views().len() - 1 + 1,
        "the scout and the ghost"
    );
    let ghost_motion = m.iter().find(|m| m.player == crate::looks::GHOST_COLOUR);
    let ghost_motion = ghost_motion.expect("a motion in the ghost colour");
    assert!(ghost_motion.id.index() >= crate::looks::first_wreck_slot(&snap));
    assert_eq!(ghost_motion.id.generation(), u32::MAX - 1);
    // On the minimap it is a ghost dot at its middle.
    let dots: Vec<_> = minimap_dots(&snap, &book).collect();
    assert_eq!(dots.iter().filter(|d| d.1 == Dot::Ghost).count(), 1);

    // It is destroyed while nobody looks: the ghost stays, the player does not know.
    let t = w.tick();
    w.schedule(t, ENEMY, Command::Despawn { id: refinery })
        .unwrap();
    run(&mut w, 3);
    let snap = SimSnapshot::capture(&w);
    book.update(&snap, &l);
    assert_eq!(book.drawn(&snap).count(), 1, "still remembered");

    // The scout comes back and sees the empty place: the ghost goes.
    send_scout(&mut w, 11, 10);
    run(&mut w, 700);
    let snap = SimSnapshot::capture(&w);
    assert_eq!(
        snap.visibility(Cell::new(14, 8)),
        sf_sim::Visibility::Visible
    );
    book.update(&snap, &l);
    assert_eq!(book.len(), 0, "seen empty: removed");
    assert_eq!(book.drawn(&snap).count(), 0);
}

#[test]
fn a_ghost_is_not_removed_while_its_cells_are_still_fog_and_goes_back_to_the_real_thing() {
    let mut w = ghost_world();
    let refinery = w.occupant_at(Cell::new(14, 8)).unwrap();
    let mut book = GhostBook::new();
    let l = looks();
    book.update(&SimSnapshot::capture(&w), &l);
    send_scout(&mut w, 40, 40);
    run(&mut w, 600);
    let snap = SimSnapshot::capture(&w);
    book.update(&snap, &l);
    assert_eq!(book.drawn(&snap).count(), 1);
    // Many more ticks in fog: it stays.
    run(&mut w, 50);
    let snap = SimSnapshot::capture(&w);
    book.update(&snap, &l);
    assert_eq!(book.drawn(&snap).count(), 1);
    // Back in sight with the structure still standing: seen for real, no ghost drawn, record kept.
    send_scout(&mut w, 11, 10);
    run(&mut w, 700);
    let snap = SimSnapshot::capture(&w);
    assert!(snap.seen(refinery));
    book.update(&snap, &l);
    assert_eq!(book.len(), 1);
    assert_eq!(book.drawn(&snap).count(), 0);
}

#[test]
fn units_and_own_structures_leave_no_ghosts() {
    let mut w = world(true);
    spawn(&mut w, LOCAL_PLAYER, "scout", 10, 10);
    spawn(&mut w, ENEMY, "light_tank", 13, 10);
    place(&mut w, LOCAL_PLAYER, "power_plant", 8, 20);
    w.step();
    let mut book = GhostBook::new();
    book.update(&SimSnapshot::capture(&w), &looks());
    assert!(book.is_empty(), "an enemy unit and an own structure: none");
    send_scout(&mut w, 40, 40);
    run(&mut w, 600);
    let snap = SimSnapshot::capture(&w);
    book.update(&snap, &looks());
    assert!(book.is_empty());
    assert_eq!(snap.views().len(), 3);
}

#[test]
fn a_new_game_or_an_earlier_tick_clears_the_book() {
    let mut w = ghost_world();
    let mut book = GhostBook::new();
    let l = looks();
    run(&mut w, 5);
    book.update(&SimSnapshot::capture(&w), &l);
    assert_eq!(book.len(), 1);
    // A snapshot of an earlier tick (a restart): forgotten before the new one is read.
    let fresh = ghost_world();
    let earlier = SimSnapshot::capture(&fresh);
    assert!(earlier.tick() < w.tick());
    let mut other = ghost_world();
    run(&mut other, 0);
    book.update(&earlier, &l);
    // The record is the new world's own refinery, seen: one record, written after the clear.
    assert_eq!(book.len(), 1);
    book.clear();
    assert!(book.is_empty());
    // A world that loses its vision rules (a no-fog scene) empties it too.
    book.update(&SimSnapshot::capture(&w), &l);
    assert_eq!(book.len(), 1);
    let no_rules = SimSnapshot::capture(&world(false));
    book.update(&no_rules, &l);
    assert!(book.is_empty());
}

// ---- the minimap ---------------------------------------------------------------------------------

#[test]
fn the_minimap_shows_shroud_fog_and_visible_in_their_colours() {
    let look = sf_ui::MinimapFog {
        shroud: [1, 2, 3],
        fog_dim: 0.5,
    };
    let rgb = [200, 100, 50];
    assert_eq!(sf_ui::fogged_color(rgb, sf_ui::VISIBLE, &look), rgb);
    assert_eq!(sf_ui::fogged_color(rgb, sf_ui::SHROUD, &look), [1, 2, 3]);
    assert_eq!(sf_ui::fogged_color(rgb, sf_ui::FOG, &look), [101, 51, 27]);
    assert_eq!(
        sf_ui::fogged_color(rgb, 7, &look),
        [1, 2, 3],
        "an unknown state hides"
    );
    // The three states differ from each other for every terrain colour (a swap shows).
    for rgb in [[255, 255, 255], [10, 20, 30], [128, 128, 128]] {
        let v = sf_ui::fogged_color(rgb, sf_ui::VISIBLE, &look);
        let f = sf_ui::fogged_color(rgb, sf_ui::FOG, &look);
        let s = sf_ui::fogged_color(rgb, sf_ui::SHROUD, &look);
        assert!(v != f && f != s && v != s, "{rgb:?}");
    }
}

#[test]
fn the_minimap_states_follow_the_snapshot_bitsets_and_the_dots_follow_what_is_seen() {
    let (mut w, snap) = field();
    let (visible, explored) = snap.fog_words();
    let states = sf_ui::fog_states(W, H, visible, explored);
    for y in 0..H {
        for x in 0..W {
            let want = match snap.visibility(Cell::new(x, y)) {
                sf_sim::Visibility::Visible => sf_ui::VISIBLE,
                sf_sim::Visibility::Fog => sf_ui::FOG,
                sf_sim::Visibility::Shroud => sf_ui::SHROUD,
            };
            assert_eq!(states[(y * W + x) as usize], want, "cell {x},{y}");
        }
    }
    assert_eq!(
        states[(10 * W + 10) as usize],
        sf_ui::VISIBLE,
        "the scout's cell"
    );
    assert_eq!(
        states[(30 * W + 50) as usize],
        sf_ui::SHROUD,
        "the far enemy's cell"
    );
    // The dots: the three shown views, not the hidden enemy.
    let book = GhostBook::new();
    let dots: Vec<_> = minimap_dots(&snap, &book).collect();
    assert_eq!(dots.len(), 3);
    assert!(dots.iter().all(|(p, _)| p.cell() != (50, 30)));
    assert_eq!(dots.iter().filter(|d| d.1 == Dot::Owner(1)).count(), 1);
    assert_eq!(dots.iter().filter(|d| d.1 == Dot::Owner(0)).count(), 2);
    // And once the scout has left, the enemy in sight is gone from the dots too.
    send_scout(&mut w, 40, 40);
    run(&mut w, 600);
    let snap = SimSnapshot::capture(&w);
    let dots: Vec<_> = minimap_dots(&snap, &book).collect();
    assert!(dots.iter().all(|d| d.1 == Dot::Owner(0)), "{dots:?}");
}

// ---- when the fog is uploaded ---------------------------------------------------------------------

#[test]
fn the_fog_is_uploaded_only_when_the_version_changes_and_taken_off_without_rules() {
    assert_eq!(fog_step(None, Some(3)), FogStep::Upload(3));
    assert_eq!(fog_step(Some(3), Some(3)), FogStep::Keep);
    assert_eq!(fog_step(Some(3), Some(4)), FogStep::Upload(4));
    assert_eq!(
        fog_step(Some(4), Some(3)),
        FogStep::Upload(3),
        "any other version"
    );
    assert_eq!(fog_step(Some(3), None), FogStep::Clear);
    assert_eq!(fog_step(None, None), FogStep::Keep);
    let mut a = FogApplied {
        terrain: Some(2),
        minimap: Some(2),
    };
    a.forget();
    assert_eq!(a, FogApplied::default());
}

#[test]
fn the_snapshot_version_moves_only_when_what_is_seen_moves() {
    let (mut w, snap) = field();
    let v = snap.fog_version().unwrap();
    w.step();
    let again = SimSnapshot::capture(&w);
    assert_eq!(again.fog_version(), Some(v), "nothing moved: same picture");
    send_scout(&mut w, 40, 40);
    run(&mut w, 300);
    let moved = SimSnapshot::capture(&w);
    assert_ne!(moved.fog_version(), Some(v));
}

// ---- effects --------------------------------------------------------------------------------------

#[test]
fn an_effect_is_shown_by_the_cell_it_is_on_and_its_owner() {
    let (_w, snap) = field();
    // The scout at (10, 10) sees around it: metres 40..44. The far corner (50, 30) cells = 200 m.
    assert!(snap.shown_at_metres([42.0, 42.0], ENEMY), "a visible cell");
    assert!(
        !snap.shown_at_metres([202.0, 122.0], ENEMY),
        "a shroud cell hides an effect of the enemy"
    );
    assert!(
        snap.shown_at_metres([202.0, 122.0], LOCAL_PLAYER),
        "the local player's own effects always show"
    );
    // The cell boundaries are 4 m: the cell of metre 3.99 is 0, of 4.0 is 1.
    assert!(!snap.shown_at_metres([-0.5, 10.0], ENEMY), "off the map");
    assert!(!snap.shown_at_metres([10_000.0, 10.0], ENEMY));
    let edge = |m: f32| snap.shown_at_metres([m, 42.0], ENEMY);
    // The scout (sight 8) sees cells 2..=18 along its row; 18 -> metres 72..76, 19 -> 76..80.
    assert!(edge(75.9) && !edge(79.9), "{} {}", edge(75.9), edge(79.9));
}

fn fight_snapshot(local_involved: bool) -> SimSnapshot {
    use sf_sim::Order;
    let d = defs();
    let w = world(true);
    let mut w = crate::playfield::with_combat(w, &d);
    spawn(&mut w, LOCAL_PLAYER, "scout", 5, 5);
    let (a, b) = if local_involved {
        (LOCAL_PLAYER, ENEMY)
    } else {
        (ENEMY, PlayerId(2))
    };
    spawn(&mut w, a, "light_tank", 40, 30);
    spawn(&mut w, b, "light_tank", 44, 30);
    w.step();
    let ids: Vec<_> = w
        .snapshot()
        .iter()
        .filter(|v| v.owner == a && v.kind == kind("light_tank"))
        .map(|v| v.id)
        .collect();
    let t = w.tick();
    w.schedule(
        t,
        a,
        Command::Order {
            ids,
            order: Order::AttackMove(at(60, 30)),
            queued: false,
        },
    )
    .unwrap();
    for _ in 0..400 {
        w.step();
        if !SimSnapshot::capture(&w).projectiles().is_empty() {
            break;
        }
    }
    SimSnapshot::capture(&w)
}

#[test]
fn projectiles_in_the_shroud_are_hidden_and_the_local_players_are_not() {
    let hidden = fight_snapshot(false);
    let p = hidden
        .projectiles()
        .first()
        .copied()
        .expect("a projectile in flight");
    assert_eq!(p.player, ENEMY);
    assert!(
        !hidden.projectile_shown(&p),
        "an enemy shell far out of sight"
    );
    let mine = fight_snapshot(true);
    let p = mine
        .projectiles()
        .first()
        .copied()
        .expect("a projectile in flight");
    assert_eq!(p.player, LOCAL_PLAYER);
    assert!(mine.projectile_shown(&p), "the local player's own shell");
    // A world without vision rules shows every shell.
    assert!(SimSnapshot::capture(&world(false)).projectiles().is_empty());
}
