//! The visual effects of the game (WP5.6a): what the events of [`crate::fx_events`], the wrecks and the
//! projectiles in flight make on the screen.
//!
//! [`FxPlayer`] is presentation state on the main thread. Once per frame:
//!
//! 1. [`FxPlayer::take`] moves the sim thread's queued events in ([`SimHandle::drain_fx`]);
//! 2. [`FxPlayer::frame`] releases the events whose tick the picture has reached (an event of tick `n` is
//!    shown once the snapshot's tick is past `n`, so a flash is in step with the interpolation), turns them
//!    into bursts of the particle pool by `assets/config/effects.ron`, lets wrecks give off smoke and
//!    projectiles leave their tracers, moves the pool on and writes the frame's
//!    [`EffectInstance`]s, which [`sf_render::Renderer::set_effects`] draws in one call.
//!
//! Which burst a weapon makes is looked up by the weapon's kind number in the definitions (`weapon_defs.ron`),
//! then its damage type, then `by_weapon` (the weapon's id). The audio hook is [`FxPlayer::sound_name`]: the
//! dotted event name of the effect an event makes, which WP11.7 maps to the Red Alert sounds.
//!
//! Nothing here reaches the simulation. `docs/rules/effects.md` has the rules.

use std::collections::BTreeMap;

use sf_data::damage::Damage;
use sf_data::effects::{EffectDef, Effects, EffectsError, Limits};
use sf_data::units::UnitDefs;
use sf_math::render_bridge::{METRES_PER_CELL, cells_to_metres, vec2_cells_to_metres};
use sf_render::Ground;
use sf_render::effects::{
    Burst, BurstDef, EffectInstance, EmitView, ParticlePool, ZoomBand, tracer_dots,
};
use sf_sim::ProjectileKind;

use crate::fx_events::{FxEvent, FxKind, NO_SHOOTER};
use crate::sim_snapshot::SimSnapshot;
use crate::sim_thread::SimHandle;

/// The longest frame time the particles are moved by, seconds (a stall does not fling them across the map).
const MAX_DT: f32 = 0.1;

/// Where the anchors of the drawn models are (WP5.6b): the one question the effects ask the picture, so
/// that a flash starts at the barrel the model has, not at a guess from its length. The runner answers it
/// from the instancer (`Instancer::anchor_world`); a game without models, and most tests, use
/// [`NoAnchors`].
pub trait Anchors {
    /// The world position of the `n`-th anchor whose name starts with `prefix` on the unit in `slot` (the
    /// index half of its id), as the unit is drawn at `alpha`. `None` when there is no such unit or
    /// anchor, and the effect then falls back to the rule of `effects.ron` (`muzzle`).
    fn anchor(&self, slot: u32, prefix: &str, n: usize, alpha: f32) -> Option<[f32; 3]>;
}

/// No model has anchors: every flash uses the bounding-box rule of `effects.ron`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoAnchors;

impl Anchors for NoAnchors {
    fn anchor(&self, _: u32, _: &str, _: usize, _: f32) -> Option<[f32; 3]> {
        None
    }
}

/// The anchors of the units the instancer drew (WP5.6b): [`Instancer::anchor_world`] on the terrain
/// height of the game, for the slots of live entities only. A slot from `first_wreck_slot` on is a wreck's
/// (`crate::looks::first_wreck_slot`): a shooter that has died since it fired is not found there, so a
/// dead shooter's flash is never started on the wreck that took its slot number.
pub struct DrawnAnchors<'a> {
    pub instancer: &'a sf_render::Instancer,
    pub ground: &'a dyn Ground,
    pub first_wreck_slot: u32,
}

impl Anchors for DrawnAnchors<'_> {
    fn anchor(&self, slot: u32, prefix: &str, n: usize, alpha: f32) -> Option<[f32; 3]> {
        if slot >= self.first_wreck_slot {
            return None;
        }
        self.instancer.anchor_world(slot, prefix, n, alpha, |x, z| {
            self.ground.height(f64::from(x), f64::from(z)) as f32
        })
    }
}

/// The anchor name a flash starts at: the node name the art brief gives a barrel's end. Like the `turret` and `barrel`
/// part names of the glb loader it is a convention of the model files, not a tunable number, so it stays in code
/// (`sf_render::glb::ANCHOR_PREFIXES` is the loader's side of the same names).
pub const MUZZLE_PREFIX: &str = "muzzle";

/// What a frame knows of the camera and the zoom.
#[derive(Clone, Copy, Debug)]
pub struct FxFrame {
    /// The frame's view-projection.
    pub view_proj: sf_render::glam::Mat4,
    /// Half the viewport height over `tan(fov_y / 2)`.
    pub focal_px: f32,
    /// The viewport in physical pixels.
    pub viewport: [f32; 2],
    /// The model weight of the model/icon cross-fade (`1` models only, `0` icons only).
    pub model_weight: f32,
    /// The apparent size of the 2 m reference unit, pixels.
    pub reference_px: f32,
    /// The side of an icon, pixels.
    pub icon_px: f32,
    /// Seconds since the last frame.
    pub dt: f32,
    /// How far between the snapshot's last two states the picture is, `0..=1`.
    pub alpha: f32,
}

/// What the effects counted, for the F1 panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FxCounters {
    /// Events taken from the sim thread's queue.
    pub taken: u64,
    /// Events turned into bursts.
    pub shown: u64,
    /// Events dropped for being older than `max_event_age_ticks` when their turn came.
    pub expired: u64,
    /// Events waiting for the picture to reach their tick.
    pub pending: u32,
    /// Tracer dots not drawn because the dot budget was spent.
    pub tracer_dots_dropped: u64,
    /// Instances in the last frame (particles and tracer dots).
    pub instances: u32,
    /// Tracer dots among them.
    pub tracer_dots: u32,
    /// Flashes that started at a muzzle anchor of the model (WP5.6b).
    pub flashes_at_anchor: u64,
    /// Flashes that used the bounding-box rule (no anchor: another model, or no model drawn).
    pub flashes_by_rule: u64,
    /// Damaged objects that gave off smoke or fire in the last frame (WP6.8).
    pub damage_sources: u32,
    /// Damaged objects skipped for being past `damage.ron`'s `max_sources` in a frame, in all.
    pub damage_sources_capped: u64,
    /// Puffs of smoke and fire made, in all.
    pub damage_puffs: u64,
    /// Puffs that started at an `fx_*` anchor of the drawn model.
    pub damage_puffs_at_anchor: u64,
    /// Puffs that started by the rule of `damage.ron` (no anchor: another model, or no model drawn).
    pub damage_puffs_by_rule: u64,
}

/// `effects.ron` resolved against the definitions: names are numbers, links are lists of numbers.
struct Tables {
    effects: Effects,
    /// The burst of each effect, by index in the file's (sorted) effect order, and its particle count.
    bursts: Vec<(BurstDef, u32)>,
    /// By weapon kind number: the effects of a shot and of a hit; empty for a kind the definitions lack.
    fire: Vec<Vec<usize>>,
    impact: Vec<Vec<usize>>,
    /// By weapon kind: whether the weapon has a splash (a hit with no victim is then still a blast).
    splash: Vec<bool>,
    miss: Vec<usize>,
    death_infantry: Vec<usize>,
    death_small: Vec<usize>,
    death_vehicle: Vec<usize>,
    death_structure: Vec<usize>,
    wreck_smoke: usize,
    trail: [Vec<usize>; 4],
}

impl Tables {
    fn new(effects: Effects, defs: &UnitDefs) -> Result<Tables, EffectsError> {
        effects.check_against(defs)?;
        let index: BTreeMap<&str, usize> = effects
            .effects
            .keys()
            .enumerate()
            .map(|(i, k)| (k.as_str(), i))
            .collect();
        let resolve =
            |names: &[String]| -> Vec<usize> { names.iter().map(|n| index[n.as_str()]).collect() };
        let bursts = effects
            .effects
            .values()
            .map(|e: &EffectDef| (BurstDef::from(e), e.count))
            .collect();
        let slots = defs
            .weapons()
            .iter()
            .map(|w| usize::from(w.kind) + 1)
            .max()
            .unwrap_or(0);
        let (mut fire, mut impact, mut splash) = (
            vec![Vec::new(); slots],
            vec![Vec::new(); slots],
            vec![false; slots],
        );
        for w in defs.weapons() {
            let k = usize::from(w.kind);
            fire[k] = resolve(effects.fire_of(&w.id, &w.damage_type));
            impact[k] = resolve(effects.impact_of(&w.id, &w.damage_type));
            splash[k] = w.splash_radius.to_fx() != sf_math::Fx::ZERO;
        }
        let t = &effects.tracers;
        let trail = [&t.straight, &t.arc, &t.guided, &t.falling].map(|t| resolve(&t.trail));
        Ok(Tables {
            miss: resolve(&effects.miss),
            death_infantry: resolve(&effects.death.infantry),
            death_small: resolve(&effects.death.small),
            death_vehicle: resolve(&effects.death.vehicle),
            death_structure: resolve(&effects.death.structure),
            wreck_smoke: index[effects.wreck_smoke.effect.as_str()],
            trail,
            bursts,
            fire,
            impact,
            splash,
            effects,
        })
    }

    /// The list of effects an event makes.
    fn list(&self, ev: &FxEvent) -> &[usize] {
        let weapon = usize::from(ev.weapon);
        match ev.kind {
            FxKind::Fire { .. } => self.fire.get(weapon).map_or(&[], Vec::as_slice),
            FxKind::Impact { hit, splash, .. } => {
                let blast = self.splash.get(weapon).copied().unwrap_or(false);
                if !hit && !splash && !blast {
                    &self.miss
                } else {
                    self.impact.get(weapon).map_or(&[], Vec::as_slice)
                }
            }
            FxKind::Death { structure, .. } => {
                if structure {
                    &self.death_structure
                } else {
                    let cells = ev.size / METRES_PER_CELL;
                    let d = &self.effects.death;
                    if cells < d.infantry_below_cells {
                        &self.death_infantry
                    } else if cells < d.small_below_cells {
                        &self.death_small
                    } else {
                        &self.death_vehicle
                    }
                }
            }
        }
    }

    fn def(&self, i: usize) -> &EffectDef {
        self.effects
            .effects
            .values()
            .nth(i)
            .expect("an index made from the same file")
    }
}

/// The effects of the game. See the module documentation.
pub struct FxPlayer {
    tables: Tables,
    pool: ParticlePool,
    /// Events taken from the sim thread whose tick the picture has not reached.
    pending: Vec<FxEvent>,
    /// Scratch for the events released in a frame (reused).
    ready: Vec<FxEvent>,
    /// The events the last [`frame`](FxPlayer::frame) showed (not the expired ones): what the audio plays.
    shown_now: Vec<FxEvent>,
    instances: Vec<EffectInstance>,
    counters: FxCounters,
    /// How many shots each `(shooter slot, weapon slot)` has had shown: a burst alternates between the
    /// model's muzzles by it.
    shots_shown: BTreeMap<(u32, u8), usize>,
    /// How damage shows (WP6.8): the thresholds, rates and rules of `damage.ron`.
    damage: Damage,
    /// The smoke and fire puff of `damage.ron`, resolved against `effects.ron` once.
    damage_puffs: (BurstDef, BurstDef),
}

impl FxPlayer {
    /// The shipped `effects.ron` against the shipped definitions.
    pub fn embedded() -> FxPlayer {
        FxPlayer::new(Effects::embedded(), &UnitDefs::embedded())
            .unwrap_or_else(|e| panic!("the shipped effects match the shipped definitions: {e}"))
    }

    /// `effects` checked against `defs`; the pool and the frame's vector are made here, once.
    pub fn new(effects: Effects, defs: &UnitDefs) -> Result<FxPlayer, EffectsError> {
        FxPlayer::with_damage(effects, defs, Damage::embedded())
    }

    /// [`FxPlayer::new`] with the damage table given (a test with a table of its own); it is checked
    /// against `effects` too.
    pub fn with_damage(
        effects: Effects,
        defs: &UnitDefs,
        damage: Damage,
    ) -> Result<FxPlayer, EffectsError> {
        damage.check_against(&effects).map_err(EffectsError)?;
        let tables = Tables::new(effects, defs)?;
        let burst = |name: &str| BurstDef::from(&tables.effects.effects[name]);
        let damage_puffs = (
            burst(&damage.emitters.smoke_effect),
            burst(&damage.emitters.fire_effect),
        );
        let l = &tables.effects.limits;
        Ok(FxPlayer {
            pool: ParticlePool::new(l),
            pending: Vec::with_capacity(crate::fx_events::FX_QUEUE_CAP),
            ready: Vec::with_capacity(crate::fx_events::FX_QUEUE_CAP),
            shown_now: Vec::with_capacity(crate::fx_events::FX_QUEUE_CAP),
            instances: Vec::with_capacity((l.max_particles + l.max_tracer_dots) as usize),
            counters: FxCounters::default(),
            shots_shown: BTreeMap::new(),
            damage,
            damage_puffs,
            tables,
        })
    }

    /// The budgets and zoom rules.
    pub fn limits(&self) -> &Limits {
        &self.tables.effects.limits
    }

    /// The counters of the effects.
    pub fn counters(&self) -> FxCounters {
        FxCounters {
            pending: self.pending.len() as u32,
            ..self.counters
        }
    }

    /// The pool's counters.
    pub fn pool_stats(&self) -> sf_render::effects::PoolStats {
        self.pool.stats()
    }

    /// Takes everything the sim thread has queued since the last call.
    pub fn take(&mut self, sim: &SimHandle) {
        let before = self.pending.len();
        sim.drain_fx(&mut self.pending);
        self.counters.taken += (self.pending.len() - before) as u64;
    }

    /// Puts events in directly (tests, and a game without a sim thread).
    pub fn push_events(&mut self, events: &[FxEvent]) {
        self.pending.extend_from_slice(events);
        self.counters.taken += events.len() as u64;
    }

    /// The audio event name of the effect `event` makes (the first effect of its list that has a sound),
    /// or `None` when it is silent. The names follow the dotted scheme of `docs/rules/effects.md`
    /// (`weapon.fire.<damage type>` or `.<weapon id>`, `impact.<damage type>`, `impact.miss`,
    /// `explosion.small|medium|large`, `unit.destroyed.infantry`); WP11.7's mapping hooks onto them.
    pub fn sound_name(&self, event: &FxEvent) -> Option<&str> {
        self.tables
            .list(event)
            .iter()
            .map(|&i| self.tables.def(i).sound.as_str())
            .find(|s| !s.is_empty())
    }

    /// The events the last [`frame`](Self::frame) showed, in the order they happened (the audio plays
    /// these, so a sound is in step with its flash and an expired event is silent too).
    pub fn shown(&self) -> &[FxEvent] {
        &self.shown_now
    }

    /// Forgets the shown events once the audio has taken them (a frame that does not call
    /// [`frame`](Self::frame) must not play them again).
    pub fn clear_shown(&mut self) {
        self.shown_now.clear();
    }

    /// The frame: release the events that are due, spawn, move, and write the instances to draw.
    pub fn frame(
        &mut self,
        snap: &SimSnapshot,
        ground: &dyn Ground,
        f: &FxFrame,
    ) -> &[EffectInstance] {
        self.frame_with(snap, ground, &NoAnchors, f)
    }

    /// [`FxPlayer::frame`] with the anchors of the drawn models: a flash starts at the muzzle anchor of
    /// the shooter's model when `anchors` has one.
    pub fn frame_with(
        &mut self,
        snap: &SimSnapshot,
        ground: &dyn Ground,
        anchors: &dyn Anchors,
        f: &FxFrame,
    ) -> &[EffectInstance] {
        let dt = f.dt.clamp(0.0, MAX_DT);
        let limits = self.tables.effects.limits.clone();
        let band = ZoomBand::of(f.model_weight, f.reference_px, &limits);
        self.release(snap, ground, anchors, f.alpha, band);
        self.wreck_smoke(snap, ground, band, dt);
        self.damage_smoke(snap, ground, anchors, band, f);
        self.pool.update(dt);
        let view = EmitView {
            view_proj: f.view_proj,
            focal_px: f.focal_px,
            viewport: f.viewport,
            model_weight: f.model_weight,
            icon_px: f.icon_px,
            limits: &limits,
        };
        self.pool.emit(&view, &mut self.instances);
        let before = self.instances.len();
        self.tracers(snap, ground, band, dt, &view, f.alpha);
        self.counters.tracer_dots = (self.instances.len() - before) as u32;
        self.counters.instances = self.instances.len() as u32;
        &self.instances
    }

    /// The events whose tick the picture has reached become bursts.
    fn release(
        &mut self,
        snap: &SimSnapshot,
        ground: &dyn Ground,
        anchors: &dyn Anchors,
        alpha: f32,
        band: ZoomBand,
    ) {
        let now = snap.tick().0;
        let mut batch = std::mem::take(&mut self.ready);
        batch.clear();
        let mut keep = 0;
        for i in 0..self.pending.len() {
            let ev = self.pending[i];
            if ev.tick < now {
                batch.push(ev);
            } else {
                self.pending[keep] = ev;
                keep += 1;
            }
        }
        self.pending.truncate(keep);
        let max_age = u64::from(self.tables.effects.limits.max_event_age_ticks);
        self.shown_now.clear();
        for ev in &batch {
            if now.saturating_sub(ev.tick) > max_age {
                self.counters.expired += 1;
                continue;
            }
            self.counters.shown += 1;
            self.shown_now.push(*ev);
            self.spawn(ev, ground, anchors, alpha, band);
        }
        self.ready = batch;
    }

    /// The bursts of one event.
    fn spawn(
        &mut self,
        ev: &FxEvent,
        ground: &dyn Ground,
        anchors: &dyn Anchors,
        alpha: f32,
        band: ZoomBand,
    ) {
        let [x, z] = ev.pos;
        let y = ground.height(f64::from(x), f64::from(z)) as f32;
        let (position, radius_m) = match ev.kind {
            FxKind::Fire { .. } if band == ZoomBand::Strategic => {
                // Icons only: a shot makes nothing, so nothing is looked up (a fight seen from the whole map is
                // hundreds of shots a second) and no flash is counted.
                return;
            }
            FxKind::Fire { slot } => {
                // The flash starts at the muzzle anchor of the shooter's drawn model, the one its burst is
                // on (a burst alternates between `muzzle_l` and `muzzle_r`); without one, out along the
                // mount's facing by the rule of `effects.ron` (the test pack, set v3, placeholder shapes).
                let key = (ev.shooter, slot);
                let shown = *self.shots_shown.get(&key).unwrap_or(&0);
                let at = (ev.shooter != NO_SHOOTER)
                    .then(|| {
                        anchors.anchor(
                            ev.shooter,
                            MUZZLE_PREFIX,
                            usize::from(slot).wrapping_add(shown),
                            alpha,
                        )
                    })
                    .flatten();
                if ev.shooter != NO_SHOOTER {
                    self.shots_shown.insert(key, shown.wrapping_add(1));
                }
                match at {
                    Some(p) => {
                        self.counters.flashes_at_anchor += 1;
                        (p, 0.0)
                    }
                    None => {
                        self.counters.flashes_by_rule += 1;
                        let m = &self.tables.effects.muzzle;
                        let reach = (m.reach_fraction * ev.size).max(m.min_reach_m);
                        let (s, c) =
                            (f32::from(ev.facing) * (std::f32::consts::TAU / 256.0)).sin_cos();
                        let (mx, mz) = (x + c * reach, z + s * reach);
                        let my = ground.height(f64::from(mx), f64::from(mz)) as f32
                            + m.height_fraction * ev.size;
                        ([mx, my, mz], 0.0)
                    }
                }
            }
            FxKind::Impact { .. } => {
                let pl = &self.tables.effects.placement;
                (
                    [x, y + pl.hit_height_m, z],
                    (ev.size * pl.impact_radius_fraction).max(pl.min_burst_radius_m),
                )
            }
            FxKind::Death { .. } => {
                // A little higher than a hit: the blast of a body starts above the wreck it leaves.
                let pl = &self.tables.effects.placement;
                (
                    [x, y + pl.hit_height_m + pl.death_lift_m, z],
                    (ev.size * pl.death_radius_fraction).max(pl.min_burst_radius_m),
                )
            }
        };
        let t = &self.tables;
        let limits = &t.effects.limits;
        let death = matches!(ev.kind, FxKind::Death { .. });
        if band == ZoomBand::Strategic {
            if death {
                // One flash, so that a battle stays visible from the overview.
                let first = t
                    .list(ev)
                    .iter()
                    .map(|&i| &t.bursts[i])
                    .find(|(b, _)| b.glow);
                if let Some((def, _)) = first {
                    self.pool.spawn(&Burst {
                        position,
                        radius_m: 0.0,
                        count: 1,
                        def: *def,
                        flash: true,
                    });
                }
            }
            return;
        }
        for &i in t.list(ev) {
            let (def, count) = t.bursts[i];
            self.pool.spawn(&Burst {
                position,
                radius_m,
                count: band.count(count, limits),
                def,
                flash: false,
            });
        }
    }

    /// Wrecks and rubble give off smoke while they lie, less over the end of their life.
    fn wreck_smoke(&mut self, snap: &SimSnapshot, ground: &dyn Ground, band: ZoomBand, dt: f32) {
        if band == ZoomBand::Strategic {
            return;
        }
        let t = &self.tables;
        let w = &t.effects.wreck_smoke;
        let (def, _) = t.bursts[t.wreck_smoke];
        let density = if band == ZoomBand::Middle {
            t.effects.limits.middle_factor
        } else {
            1.0
        };
        for wreck in snap.wrecks() {
            if wreck.ticks_left == 0 || wreck.lifetime == 0 {
                continue;
            }
            // Full until the last `fade_fraction` of its life, then down to nothing.
            let tail = (wreck.lifetime as f32 * w.fade_fraction).max(1.0);
            let fade = (wreck.ticks_left as f32 / tail).min(1.0);
            let expected = w.per_second * dt * fade * density;
            let mut n = expected.floor() as u32;
            if self.pool.chance(expected.fract()) {
                n += 1;
            }
            if n == 0 {
                continue;
            }
            let [x, z] = vec2_cells_to_metres(wreck.pos);
            let y =
                ground.height(f64::from(x), f64::from(z)) as f32 + t.effects.placement.hit_height_m;
            let length_m = match wreck.footprint {
                Some(fp) => f32::from(fp.width.max(fp.height)) * METRES_PER_CELL,
                None => cells_to_metres(wreck.size),
            };
            self.pool.spawn(&Burst {
                position: [x, y, z],
                radius_m: w.radius_fraction * length_m,
                count: n,
                def,
                flash: false,
            });
        }
    }

    /// Smoke on damaged objects and smoke and fire on heavily damaged ones (WP6.8), from the snapshot's
    /// hit points: no event and no memory, so an object repaired above a threshold stops at once and a
    /// dead one is gone from the snapshot (its wreck's smoke takes over). Puffs start at the drawn
    /// model's `fx_smoke` / `fx_fire` anchor, else by the rule of `damage.ron` above the middle of the
    /// body. Nothing at strategic zoom, the middle factor at middle zoom; objects off the screen are
    /// skipped, and at most `max_sources` give off puffs in one frame.
    fn damage_smoke(
        &mut self,
        snap: &SimSnapshot,
        ground: &dyn Ground,
        anchors: &dyn Anchors,
        band: ZoomBand,
        f: &FxFrame,
    ) {
        self.counters.damage_sources = 0;
        if band == ZoomBand::Strategic {
            return;
        }
        let d = &self.damage;
        let density = if band == ZoomBand::Middle {
            self.tables.effects.limits.middle_factor
        } else {
            1.0
        };
        let margin = d.limits.cull_margin;
        let (w, h) = (f.viewport[0], f.viewport[1]);
        for v in snap.views() {
            let Some(rates) = d.rates(snap.damage_stage(d, v)) else {
                continue;
            };
            let Some(stats) = snap.stats_for(v) else {
                continue;
            };
            // Where the body is, and how long: a structure's longer footprint side, a unit's size.
            let (length_m, structure) = match stats.footprint {
                Some(fp) => (f32::from(fp.width.max(fp.height)) * METRES_PER_CELL, true),
                None => (cells_to_metres(stats.size), false),
            };
            if !structure && length_m < d.emitters.min_body_cells * METRES_PER_CELL {
                continue;
            }
            let (a, b) = (
                vec2_cells_to_metres(v.prev_pos),
                vec2_cells_to_metres(v.pos),
            );
            let (x, z) = (
                a[0] + (b[0] - a[0]) * f.alpha,
                a[1] + (b[1] - a[1]) * f.alpha,
            );
            let y = ground.height(f64::from(x), f64::from(z)) as f32;
            // Off the screen (or behind the camera): nothing to see, so no lookup and no puffs.
            let on_screen = sf_render::pick::project_px(
                &f.view_proj,
                sf_render::glam::Vec3::new(x, y, z),
                f.viewport,
            )
            .is_some_and(|(p, _)| {
                p[0] >= -margin * w
                    && p[0] <= w * (1.0 + margin)
                    && p[1] >= -margin * h
                    && p[1] <= h * (1.0 + margin)
            });
            if !on_screen {
                continue;
            }
            if self.counters.damage_sources >= d.limits.max_sources {
                self.counters.damage_sources_capped += 1;
                continue;
            }
            self.counters.damage_sources += 1;
            let fb = &d.fallback;
            let by_rule = [
                x,
                y + (fb.height_fraction * length_m).max(fb.min_height_m),
                z,
            ];
            let rule_radius = (fb.radius_fraction * length_m).max(fb.min_radius_m);
            let slot = v.id.index();
            for (rate, prefix, def) in [
                (
                    rates.smoke_per_s,
                    &d.emitters.smoke_anchor,
                    self.damage_puffs.0,
                ),
                (
                    rates.fire_per_s,
                    &d.emitters.fire_anchor,
                    self.damage_puffs.1,
                ),
            ] {
                if rate <= 0.0 {
                    continue;
                }
                let expected = rate * f.dt.clamp(0.0, MAX_DT) * density;
                let mut n = expected.floor() as u32;
                if self.pool.chance(expected.fract()) {
                    n += 1;
                }
                if n == 0 {
                    continue;
                }
                let (position, radius_m) = match anchors.anchor(slot, prefix, 0, f.alpha) {
                    Some(p) => {
                        self.counters.damage_puffs_at_anchor += u64::from(n);
                        (
                            [p[0], p[1] + d.emitters.anchor_lift_m, p[2]],
                            d.emitters.anchor_radius_m,
                        )
                    }
                    None => {
                        self.counters.damage_puffs_by_rule += u64::from(n);
                        (by_rule, rule_radius)
                    }
                };
                self.counters.damage_puffs += u64::from(n);
                self.pool.spawn(&Burst {
                    position,
                    radius_m,
                    count: n,
                    def,
                    flash: false,
                });
            }
        }
    }

    /// The tracers of the projectiles in flight, appended to the frame's instances, and the puffs a
    /// projectile leaves behind.
    fn tracers(
        &mut self,
        snap: &SimSnapshot,
        ground: &dyn Ground,
        band: ZoomBand,
        dt: f32,
        view: &EmitView<'_>,
        alpha: f32,
    ) {
        if band == ZoomBand::Strategic {
            return;
        }
        let a = alpha.clamp(0.0, 1.0);
        let lerp = |p: f32, c: f32| p + (c - p) * a;
        let density = if band == ZoomBand::Middle {
            view.limits.middle_factor
        } else {
            1.0
        };
        let mut budget = view.limits.max_tracer_dots as usize;
        for p in snap.projectiles() {
            let (slot, tracer) = match p.kind {
                ProjectileKind::Straight => (0, &self.tables.effects.tracers.straight),
                ProjectileKind::Arc => (1, &self.tables.effects.tracers.arc),
                ProjectileKind::Guided => (2, &self.tables.effects.tracers.guided),
                ProjectileKind::Falling => (3, &self.tables.effects.tracers.falling),
                ProjectileKind::Instant => continue,
            };
            let [px, pz] = vec2_cells_to_metres(p.pos);
            let [qx, qz] = vec2_cells_to_metres(p.prev_pos);
            let (hx, hz) = (lerp(qx, px), lerp(qz, pz));
            let height = lerp(cells_to_metres(p.prev_height), cells_to_metres(p.height));
            let lift = if height > 0.0 {
                0.0
            } else {
                self.tables.effects.placement.projectile_lift_m
            };
            let ground_y = ground.height(f64::from(hx), f64::from(hz)) as f32;
            let head = [hx, ground_y + height + lift, hz];
            // The way it flies: along its ground track, climbing or falling with its height.
            let dh = cells_to_metres(p.height) - cells_to_metres(p.prev_height);
            let (dx, dz) = (px - qx, pz - qz);
            let len = (dx * dx + dz * dz + dh * dh).sqrt();
            let dir = if len > 1e-4 {
                [dx / len, dh / len, dz / len]
            } else {
                [1.0, 0.0, 0.0]
            };
            if budget < tracer.dots as usize {
                self.counters.tracer_dots_dropped += u64::from(tracer.dots);
            } else {
                budget -= tracer_dots(tracer, head, dir, view, budget, &mut self.instances);
            }
            // The puffs of a guided one.
            if tracer.trail_per_s > 0.0 {
                let expected = tracer.trail_per_s * dt * density;
                let mut n = expected.floor() as u32;
                if self.pool.chance(expected.fract()) {
                    n += 1;
                }
                if n > 0 {
                    for &i in &self.tables.trail[slot] {
                        let (def, _) = self.tables.bursts[i];
                        self.pool.spawn(&Burst {
                            position: head,
                            radius_m: 0.0,
                            count: n,
                            def,
                            flash: false,
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_render::FlatGround;
    use sf_render::glam::Vec3;
    use sf_sim::{ImpactCause, ProjectileKind, World, WorldConfig};

    fn player() -> FxPlayer {
        FxPlayer::embedded()
    }

    /// A snapshot at tick `ticks` of an empty world.
    fn snapshot_at(ticks: u32) -> SimSnapshot {
        let mut world = World::new(WorldConfig::new(1).with_map_size(8, 8)).unwrap();
        for _ in 0..ticks {
            world.step();
        }
        SimSnapshot::capture(&world)
    }

    fn weapon(id: &str) -> u16 {
        UnitDefs::embedded().weapon(id).unwrap().kind
    }

    fn fire(weapon: u16) -> FxEvent {
        FxEvent {
            tick: 0,
            kind: FxKind::Fire { slot: 0 },
            weapon,
            player: 0,
            pos: [12.0, 12.0],
            facing: 0,
            size: 6.0,
            shooter: crate::fx_events::NO_SHOOTER,
        }
    }

    fn hit(weapon: u16, hit: bool, splash: bool) -> FxEvent {
        FxEvent {
            kind: FxKind::Impact {
                cause: ImpactCause::Proximity,
                hit,
                splash,
            },
            size: 0.0,
            ..fire(weapon)
        }
    }

    fn death(size_m: f32, structure: bool) -> FxEvent {
        FxEvent {
            kind: FxKind::Death {
                wreck: true,
                structure,
            },
            weapon: 0,
            size: size_m,
            ..fire(0)
        }
    }

    /// A camera looking at (12, 12) from the south at 40 m, wide.
    fn frame(model_weight: f32, reference_px: f32) -> FxFrame {
        let eye = Vec3::new(12.0, 30.0, 40.0);
        let view = sf_render::glam::camera::rh::view::look_at_mat4(
            eye,
            Vec3::new(12.0, 0.0, 12.0),
            Vec3::Y,
        );
        let proj =
            sf_render::glam::camera::rh::proj::directx::perspective(0.8, 16.0 / 9.0, 0.1, 500.0);
        FxFrame {
            view_proj: proj * view,
            focal_px: 0.5 * 540.0 / (0.4_f32).tan(),
            viewport: [960.0, 540.0],
            model_weight,
            reference_px,
            icon_px: 18.0,
            dt: 0.05,
            alpha: 1.0,
        }
    }

    fn spawned(p: &FxPlayer) -> u64 {
        p.pool_stats().spawned
    }

    /// Every name of the sound table, pinned: the audio of WP11.7 maps these.
    #[test]
    fn the_event_names_follow_the_scheme() {
        let p = player();
        let name = |e: FxEvent| p.sound_name(&e).map(str::to_owned);
        let w = weapon;
        // Firing: the weapon id where it has an override, else the damage type.
        assert_eq!(
            name(fire(w("rifle"))).as_deref(),
            Some("weapon.fire.bullet")
        );
        assert_eq!(
            name(fire(w("long_rifle"))).as_deref(),
            Some("weapon.fire.precision")
        );
        assert_eq!(
            name(fire(w("cannon"))).as_deref(),
            Some("weapon.fire.cannon")
        );
        assert_eq!(
            name(fire(w("twin_cannon"))).as_deref(),
            Some("weapon.fire.cannon")
        );
        assert_eq!(
            name(fire(w("rocket"))).as_deref(),
            Some("weapon.fire.rocket")
        );
        assert_eq!(
            name(fire(w("battery_shell"))).as_deref(),
            Some("weapon.fire.shell")
        );
        assert_eq!(
            name(fire(w("howitzer"))).as_deref(),
            Some("weapon.fire.howitzer")
        );
        assert_eq!(
            name(fire(w("satchel_charge"))).as_deref(),
            Some("weapon.fire.demolition")
        );
        // Hits: the damage type; no victim and no splash is a miss.
        assert_eq!(
            name(hit(w("rifle"), true, false)).as_deref(),
            Some("impact.bullet")
        );
        assert_eq!(
            name(hit(w("cannon"), true, true)).as_deref(),
            Some("impact.cannon")
        );
        assert_eq!(
            name(hit(w("howitzer"), false, false)).as_deref(),
            Some("impact.shell"),
            "a shell with a splash is a blast on the ground"
        );
        assert_eq!(
            name(hit(w("cannon"), false, false)).as_deref(),
            Some("impact.miss")
        );
        assert_eq!(
            name(hit(w("rocket"), false, false)).as_deref(),
            Some("impact.miss")
        );
        // Deaths: by size and kind.
        assert_eq!(
            name(death(1.6, false)).as_deref(),
            Some("unit.destroyed.infantry")
        );
        assert_eq!(name(death(4.8, false)).as_deref(), Some("explosion.small"));
        assert_eq!(name(death(6.0, false)).as_deref(), Some("explosion.medium"));
        assert_eq!(name(death(8.0, true)).as_deref(), Some("explosion.large"));
        // Every shipped weapon names a fire and a hit.
        for weapon in UnitDefs::embedded().weapons() {
            assert!(
                name(fire(weapon.kind)).unwrap().starts_with("weapon.fire."),
                "{}",
                weapon.id
            );
            assert!(
                name(hit(weapon.kind, true, false))
                    .unwrap()
                    .starts_with("impact."),
                "{}",
                weapon.id
            );
        }
        // A weapon the definitions do not have makes nothing and says nothing.
        assert_eq!(name(fire(4000)), None);
    }

    #[test]
    fn an_event_is_shown_once_the_picture_has_passed_its_tick() {
        let mut p = player();
        let ground = FlatGround::default();
        let f = frame(1.0, 200.0);
        p.push_events(&[fire(weapon("cannon"))]);
        // The snapshot is at tick 0: the event is of tick 0, so it waits.
        p.frame(&snapshot_at(0), &ground, &f);
        assert_eq!(spawned(&p), 0);
        assert_eq!(p.counters().pending, 1);
        // One tick later it is shown, and only once.
        p.frame(&snapshot_at(1), &ground, &f);
        assert_eq!(p.counters().pending, 0);
        let first = spawned(&p);
        assert!(first > 0);
        p.frame(&snapshot_at(1), &ground, &f);
        assert_eq!(spawned(&p), first);
        assert_eq!(p.counters().shown, 1);
    }

    #[test]
    fn an_event_that_is_too_old_is_dropped_and_counted() {
        let mut p = player();
        let ground = FlatGround::default();
        let f = frame(1.0, 200.0);
        assert_eq!(p.limits().max_event_age_ticks, 40);
        // 41 ticks old: gone. 40 ticks old: shown.
        p.push_events(&[fire(weapon("cannon"))]);
        p.frame(&snapshot_at(41), &ground, &f);
        assert_eq!((p.counters().expired, spawned(&p)), (1, 0));
        p.push_events(&[fire(weapon("cannon"))]);
        p.frame(&snapshot_at(40), &ground, &f);
        assert_eq!(p.counters().expired, 1);
        assert!(spawned(&p) > 0);
    }

    /// The number of particles a burst makes at each zoom: all, half (rounded up), and for a fire or a hit
    /// none at strategic zoom; a death keeps one flash.
    #[test]
    fn density_follows_the_zoom_and_a_death_keeps_a_flash() {
        let ground = FlatGround::default();
        let count = |ev: FxEvent, f: FxFrame| {
            let mut p = player();
            p.push_events(&[ev]);
            p.frame(&snapshot_at(1), &ground, &f);
            spawned(&p)
        };
        let cannon = fire(weapon("cannon")); // flash_cannon 3 + smoke_muzzle 2
        assert_eq!(count(cannon, frame(1.0, 200.0)), 3 + 2, "close");
        assert_eq!(
            count(cannon, frame(1.0, 30.0)),
            2 + 1,
            "middle: half, rounded up"
        );
        assert_eq!(
            count(cannon, frame(0.5, 200.0)),
            2 + 1,
            "the cross-fade is middle too"
        );
        assert_eq!(count(cannon, frame(0.0, 5.0)), 0, "strategic: nothing");
        let tank = death(6.0, false); // explosion_medium 9 + smoke_death 5
        assert_eq!(count(tank, frame(1.0, 200.0)), 9 + 5);
        assert_eq!(count(tank, frame(1.0, 30.0)), 5 + 3);
        assert_eq!(count(tank, frame(0.0, 5.0)), 1, "strategic: one flash");
        let miss = hit(weapon("cannon"), false, false); // dust_puff 3
        assert_eq!(count(miss, frame(1.0, 200.0)), 3);
    }

    #[test]
    fn at_strategic_zoom_only_the_death_flash_is_drawn() {
        let ground = FlatGround::default();
        let events = [
            fire(weapon("cannon")),
            hit(weapon("cannon"), true, false),
            death(6.0, false),
        ];
        let mut p = player();
        p.push_events(&events);
        let f = frame(0.0, 5.0);
        let drawn = p.frame(&snapshot_at(1), &ground, &f).to_vec();
        assert_eq!(drawn.len(), 1, "only the flash");
        assert!(
            drawn[0].size_px <= f.icon_px,
            "no bigger than an icon: {}",
            drawn[0].size_px
        );
        assert!(drawn[0].color[0] >= p.limits().strategic_flash_alpha * 0.9);
        // At close zoom the same events draw many.
        let mut q = player();
        q.push_events(&events);
        let close = q.frame(&snapshot_at(1), &ground, &frame(1.0, 200.0)).len();
        assert!(close > 10, "{close}");
    }

    /// A world with the game's combat in which `attacker` (owner 0) is sent at `target` (owner 1), four cells
    /// apart; stepped until a projectile is in flight, then the snapshot of that tick.
    fn projectile_in_flight(attacker: &str, target: &str, want: ProjectileKind) -> SimSnapshot {
        use sf_math::Vec2;
        use sf_sim::{Command, Order, PlayerId};
        let defs = UnitDefs::embedded();
        let world = World::new(WorldConfig::new(1).with_map_size(64, 64))
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let mut world = crate::playfield::with_combat(world, &defs);
        let kind = |role: &str| sf_sim::Kind(defs.kind_of(role).unwrap());
        let at = |x: i32| Vec2::from_ints(x, 30) + Vec2::new(sf_math::Fx::HALF, sf_math::Fx::HALF);
        let spawn = |w: &mut World, role: &str, x: i32, owner: u8| {
            w.schedule(
                w.tick(),
                PlayerId(0),
                Command::SpawnUnit {
                    kind: kind(role),
                    pos: at(x),
                    owner: PlayerId(owner),
                },
            )
            .unwrap();
        };
        spawn(&mut world, attacker, 20, 0);
        spawn(&mut world, target, 24, 1);
        world.step();
        let ids: Vec<_> = world
            .snapshot()
            .iter()
            .filter(|v| v.owner == PlayerId(0))
            .map(|v| v.id)
            .collect();
        world
            .schedule(
                world.tick(),
                PlayerId(0),
                Command::Order {
                    ids,
                    order: Order::AttackMove(at(30)),
                    queued: false,
                },
            )
            .unwrap();
        for _ in 0..400 {
            world.step();
            let snap = SimSnapshot::capture(&world);
            if snap.projectiles().iter().any(|p| p.kind == want) {
                return snap;
            }
        }
        panic!("{attacker} never fired a {want:?} projectile at {target}");
    }

    /// A camera over the first projectile of kind `want`, close.
    fn frame_over(snap: &SimSnapshot, want: ProjectileKind) -> FxFrame {
        let p = snap
            .projectiles()
            .iter()
            .find(|p| p.kind == want)
            .unwrap()
            .pos;
        let [x, z] = vec2_cells_to_metres(p);
        let eye = Vec3::new(x, 40.0, z + 40.0);
        let view =
            sf_render::glam::camera::rh::view::look_at_mat4(eye, Vec3::new(x, 0.0, z), Vec3::Y);
        let proj =
            sf_render::glam::camera::rh::proj::directx::perspective(0.8, 16.0 / 9.0, 0.1, 500.0);
        FxFrame {
            view_proj: proj * view,
            focal_px: 0.5 * 540.0 / (0.4_f32).tan(),
            viewport: [960.0, 540.0],
            model_weight: 1.0,
            reference_px: 200.0,
            icon_px: 18.0,
            dt: 0.05,
            alpha: 1.0,
        }
    }

    /// A shell in flight is a row of glowing dots (the straight tracer: four), and none at strategic zoom.
    #[test]
    fn a_projectile_in_flight_is_drawn_as_a_tracer() {
        let snap = projectile_in_flight("light_tank", "light_tank", ProjectileKind::Straight);
        let mut p = player();
        let f = frame_over(&snap, ProjectileKind::Straight);
        let dots = p.frame(&snap, &FlatGround::default(), &f).len();
        assert_eq!(dots as u32, p.counters().tracer_dots);
        assert!(dots >= 4, "four dots for a straight shell in view: {dots}");
        assert_eq!(dots % 4, 0, "four dots each: {dots}");
        // Middle zoom still draws it (the tracer is the projectile, not a burst), strategic does not.
        let mut q = player();
        let far = FxFrame {
            model_weight: 0.0,
            reference_px: 5.0,
            ..f
        };
        assert_eq!(q.frame(&snap, &FlatGround::default(), &far).len(), 0);
        // The dots are along the shell's way: the head is at its interpolated position.
        let mut r = player();
        let at_end = r
            .frame(&snap, &FlatGround::default(), &FxFrame { alpha: 1.0, ..f })
            .to_vec();
        let at_start = r
            .frame(&snap, &FlatGround::default(), &FxFrame { alpha: 0.0, ..f })
            .to_vec();
        let v = *snap
            .projectiles()
            .iter()
            .find(|p| p.kind == ProjectileKind::Straight)
            .unwrap();
        if v.pos != v.prev_pos {
            assert_ne!(
                at_end[0].position, at_start[0].position,
                "alpha moves the head"
            );
        }
    }

    /// A rocket leaves a trail of smoke puffs as well: the guided tracer's `trail_per_s`.
    #[test]
    fn a_guided_projectile_leaves_a_trail() {
        let snap = projectile_in_flight("rocket_trooper", "light_tank", ProjectileKind::Guided);
        let mut p = player();
        let f = frame_over(&snap, ProjectileKind::Guided);
        let before = spawned(&p);
        for _ in 0..20 {
            p.frame(&snap, &FlatGround::default(), &FxFrame { dt: 0.1, ..f });
        }
        // 30 puffs a second for 2 seconds of frames, at least half of them land (the chance is a draw).
        let puffs = spawned(&p) - before;
        assert!(puffs >= 30, "{puffs} puffs of smoke from the rocket");
        assert!(p.pool.positions().count() > 0);
    }

    #[test]
    fn the_muzzle_is_out_along_the_mount_and_up() {
        let ground = FlatGround::default();
        let mut p = player();
        // A unit 6 m long at (12, 12) aiming along +z (facing 64): reach 3 m, height 0.22 * 6 m.
        let ev = FxEvent {
            facing: 64,
            ..fire(weapon("rifle"))
        };
        p.push_events(&[ev]);
        p.frame(&snapshot_at(1), &ground, &frame(1.0, 200.0));
        let at = p.pool.positions().next().expect("a flash");
        assert!((at[0] - 12.0).abs() < 1e-3, "x {}", at[0]);
        assert!((at[2] - 15.0).abs() < 1e-3, "z {}", at[2]);
        assert!((at[1] - 1.32).abs() < 1e-3, "height {}", at[1]);
        // A small unit gets the least reach.
        let mut q = player();
        q.push_events(&[FxEvent {
            facing: 0,
            size: 1.0,
            ..fire(weapon("rifle"))
        }]);
        q.frame(&snapshot_at(1), &ground, &frame(1.0, 200.0));
        let at = q.pool.positions().next().unwrap();
        assert!((at[0] - 12.8).abs() < 1e-3, "min reach 0.8 m: {}", at[0]);
    }

    /// Answers the muzzle question with a fixed point and writes down what it was asked.
    struct Fixed {
        at: Option<[f32; 3]>,
        asked: std::cell::RefCell<Vec<(u32, String, usize, f32)>>,
    }

    impl Anchors for Fixed {
        fn anchor(&self, slot: u32, prefix: &str, n: usize, alpha: f32) -> Option<[f32; 3]> {
            self.asked
                .borrow_mut()
                .push((slot, prefix.to_owned(), n, alpha));
            self.at
        }
    }

    fn fixed(at: Option<[f32; 3]>) -> Fixed {
        Fixed {
            at,
            asked: Default::default(),
        }
    }

    fn shot_by(shooter: u32, slot: u8, weapon: u16) -> FxEvent {
        FxEvent {
            kind: FxKind::Fire { slot },
            shooter,
            ..fire(weapon)
        }
    }

    /// With an anchor the flash starts there, not at the bounding-box rule; the question names the shooter's
    /// slot, the prefix `muzzle` and the frame's alpha.
    #[test]
    fn a_flash_starts_at_the_muzzle_anchor_when_the_model_has_one() {
        let ground = FlatGround::default();
        let mut p = player();
        let anchors = fixed(Some([30.0, 2.5, 44.0]));
        p.push_events(&[shot_by(9, 0, weapon("rifle"))]);
        let f = FxFrame {
            alpha: 0.25,
            ..frame(1.0, 200.0)
        };
        p.frame_with(&snapshot_at(1), &ground, &anchors, &f);
        assert_eq!(*anchors.asked.borrow(), [(9, "muzzle".to_owned(), 0, 0.25)]);
        let at = p.pool.positions().next().expect("a flash");
        // Moved on by the frame's 0.05 s only (a flash does not rise); the point, not the rule's.
        assert!(
            (at[0] - 30.0).abs() < 1e-3 && (at[2] - 44.0).abs() < 1e-3,
            "{at:?}"
        );
        assert!((at[1] - 2.5).abs() < 0.2, "height {}", at[1]);
        let c = p.counters();
        assert_eq!((c.flashes_at_anchor, c.flashes_by_rule), (1, 0));
    }

    /// No anchor (another model), no shooter, or the anchor lookup refusing: the rule of `effects.ron`.
    #[test]
    fn a_model_without_a_muzzle_gets_the_bounding_box_rule() {
        let ground = FlatGround::default();
        let rule = |shooter: u32, anchors: &dyn Anchors| {
            let mut p = player();
            p.push_events(&[FxEvent {
                facing: 64,
                ..shot_by(shooter, 0, weapon("rifle"))
            }]);
            p.frame_with(&snapshot_at(1), &ground, anchors, &frame(1.0, 200.0));
            let at = p.pool.positions().next().unwrap();
            ((at[0], at[2]), p.counters())
        };
        // The rule's place for a 6 m unit at (12, 12) aiming along +z: (12, 15).
        for (shooter, anchors) in [
            (9, &fixed(None) as &dyn Anchors),
            (NO_SHOOTER, &fixed(Some([30.0, 2.5, 44.0]))),
            (9, &NoAnchors),
        ] {
            let ((x, z), c) = rule(shooter, anchors);
            assert!(
                (x - 12.0).abs() < 1e-3 && (z - 15.0).abs() < 1e-3,
                "{x} {z}"
            );
            assert_eq!((c.flashes_at_anchor, c.flashes_by_rule), (0, 1));
        }
        // A shot without a shooter never asks.
        let anchors = fixed(Some([30.0, 2.5, 44.0]));
        rule(NO_SHOOTER, &anchors);
        assert!(anchors.asked.borrow().is_empty());
    }

    /// A burst alternates between the model's muzzles: the n-th shot of a shooter's weapon slot asks for
    /// anchor `slot + n`, and another shooter or another slot counts on its own.
    #[test]
    fn a_burst_alternates_between_the_muzzles_by_shooter_and_slot() {
        let ground = FlatGround::default();
        let mut p = player();
        let anchors = fixed(Some([30.0, 2.5, 44.0]));
        let w = weapon("twin_cannon");
        let shots = [
            shot_by(4, 0, w),
            shot_by(4, 0, w),
            shot_by(4, 0, w),
            shot_by(7, 0, w),
            shot_by(4, 1, w),
            shot_by(4, 1, w),
        ];
        p.push_events(&shots);
        p.frame_with(&snapshot_at(1), &ground, &anchors, &frame(1.0, 200.0));
        let asked: Vec<(u32, usize)> = anchors
            .asked
            .borrow()
            .iter()
            .map(|(slot, _, n, _)| (*slot, *n))
            .collect();
        // Shooter 4 slot 0: 0, 1, 2 (left, right, left of two muzzles); shooter 7: 0; shooter 4 slot 1:
        // 1 + 0, 1 + 1.
        assert_eq!(asked, [(4, 0), (4, 1), (4, 2), (7, 0), (4, 1), (4, 2)]);
    }

    /// From the whole map (icons only) a shot makes no flash and asks the picture for no anchor.
    #[test]
    fn a_shot_seen_from_afar_looks_up_no_anchor_and_counts_no_flash() {
        let ground = FlatGround::default();
        let mut p = player();
        let anchors = fixed(Some([30.0, 2.5, 44.0]));
        p.push_events(&[shot_by(9, 0, weapon("rifle"))]);
        let drawn = p
            .frame_with(&snapshot_at(1), &ground, &anchors, &frame(0.0, 5.0))
            .len();
        assert_eq!(drawn, 0);
        assert!(anchors.asked.borrow().is_empty(), "no lookup from afar");
        let c = p.counters();
        assert_eq!((c.flashes_at_anchor, c.flashes_by_rule, c.shown), (0, 0, 1));
    }

    /// A death bursts `death_lift_m` higher than the same burst would at the hit height, and a hit does not.
    #[test]
    fn a_death_bursts_a_little_higher_than_a_hit() {
        let ground = FlatGround::default();
        let height = |lift: f32, ev: FxEvent| {
            let mut effects = Effects::embedded();
            effects.placement.death_lift_m = lift;
            let mut p = FxPlayer::new(effects, &UnitDefs::embedded()).unwrap();
            p.push_events(&[ev]);
            p.frame(&snapshot_at(1), &ground, &frame(1.0, 200.0));
            let ys: Vec<f32> = p.pool.positions().map(|q| q[1]).collect();
            ys.iter().sum::<f32>() / ys.len() as f32
        };
        let lift = Effects::embedded().placement.death_lift_m;
        assert_eq!(lift, 0.5, "the shipped lift");
        let d = death(6.0, false);
        assert!((height(lift, d) - height(0.0, d) - lift).abs() < 1e-4);
        let h = hit(weapon("cannon"), true, false);
        assert!(
            (height(lift, h) - height(0.0, h)).abs() < 1e-6,
            "a hit is not lifted"
        );
    }

    /// A slot from the first wreck slot on is a wreck's, never a shooter's: a shooter that died after it
    /// fired is not looked for under the wreck that took its slot number.
    #[test]
    fn drawn_anchors_do_not_answer_for_a_wreck_slot() {
        use sf_render::glb::{AxisConvention, DEFAULT_SWITCH_PX, load_glb_file, model_set};
        use sf_render::{Instancer, Motion, Style, UnitId};
        let ground = FlatGround::default();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/set_v4/veyran_heavy_tank.glb");
        let model = load_glb_file(&path, &AxisConvention::ENGINE).unwrap();
        let mut inst = Instancer::new(Style::embedded(), (64.0, 64.0));
        inst.set_models(model_set(&[model], &DEFAULT_SWITCH_PX).unwrap());
        let tank = |slot| {
            Motion::from_fixed(
                0,
                0,
                sf_math::Vec2::from_ints(5, 5),
                sf_math::Vec2::from_ints(5, 5),
                sf_math::Facing::new(0),
                sf_math::Facing::new(0),
            )
            .with_id(UnitId::from_parts(slot, 0))
        };
        // Two tanks in slots 2 and 5; the snapshot's live entities end at slot 5, so wrecks start at 6.
        inst.update_tick([tank(2), tank(5)]);
        let drawn = |first_wreck_slot| DrawnAnchors {
            instancer: &inst,
            ground: &ground,
            first_wreck_slot,
        };
        let a = drawn(6);
        let at = a.anchor(5, "muzzle", 1, 0.0).expect("the tank in slot 5");
        // 5 cells = 20 m; the muzzle is 4.2 m ahead and 1.77 m up (muzzle_r of the heavy tank).
        assert!(
            (at[0] - 24.212).abs() < 1e-3 && (at[1] - 1.769).abs() < 1e-3,
            "{at:?}"
        );
        assert!(a.anchor(2, "muzzle", 0, 0.0).is_some());
        // The same instancer, when slot 5 is a wreck's (the entities end at slot 2): nothing is found there.
        let b = drawn(3);
        assert!(b.anchor(5, "muzzle", 0, 0.0).is_none());
        assert!(b.anchor(2, "muzzle", 0, 0.0).is_some());
        // And the cut is the snapshot's: the first wreck slot is one past the last live entity.
        let snap = snapshot_at(1);
        assert_eq!(crate::looks::first_wreck_slot(&snap), 0, "an empty world");
    }

    // ---- Damage: smoke and fire (WP6.8) ----

    use sf_sim::EntityView;

    /// The views of live objects `(role, cell x, cell y, hit points)` of owner 0 (see
    /// `SimSnapshot::test_views`).
    fn damaged_views(
        items: &[(&str, i32, i32, u32)],
    ) -> (Vec<EntityView>, std::sync::Arc<sf_sim::UnitTable>) {
        let items: Vec<_> = items
            .iter()
            .map(|&(role, x, y, hp)| (role, 0, x, y, hp))
            .collect();
        SimSnapshot::test_views(&items)
    }

    fn damaged_world(items: &[(&str, i32, i32, u32)]) -> SimSnapshot {
        let (views, table) = damaged_views(items);
        SimSnapshot::of_views(views, table)
    }

    /// Runs `frames` frames of 0.1 s.
    fn run_damage(
        p: &mut FxPlayer,
        snap: &SimSnapshot,
        anchors: &dyn Anchors,
        frame: &FxFrame,
        frames: u32,
    ) {
        let ground = FlatGround::default();
        let f = FxFrame { dt: 0.1, ..*frame };
        for _ in 0..frames {
            p.frame_with(snap, &ground, anchors, &f);
        }
    }

    /// The power plant has 600 hit points: 660 thousandths is 396 of them, 330 thousandths 198. An
    /// object above the first threshold gives off nothing, at or below it smoke, at or below the second
    /// smoke and fire. Through the same `frame_with` the game calls.
    #[test]
    fn the_stage_of_the_hit_points_decides_what_an_object_gives_off() {
        let at = |hp: u32| {
            let mut p = player();
            let anchors = fixed(Some([12.0, 6.0, 12.0]));
            run_damage(
                &mut p,
                &damaged_world(&[("power_plant", 3, 3, hp)]),
                &anchors,
                &frame(1.0, 200.0),
                100,
            );
            (p.counters(), anchors.asked.borrow().clone())
        };
        let (intact, asked) = at(600);
        assert_eq!((intact.damage_puffs, intact.damage_sources), (0, 0));
        assert!(asked.is_empty(), "no lookup for an intact object");
        // 397 of 600 is 661 thousandths: still intact.
        assert_eq!(at(397).0.damage_puffs, 0);
        // 396 is 660: damaged. Smoke only: the fire anchor is never asked.
        let (damaged, asked) = at(396);
        assert_eq!(damaged.damage_sources, 1);
        assert!(damaged.damage_puffs > 0);
        assert!(asked.iter().all(|a| a.1 == "fx_smoke"), "{asked:?}");
        // 199 is 331 thousandths: still only damaged. 198 is 330: heavily damaged, smoke and fire.
        let (still, asked) = at(199);
        assert!(asked.iter().all(|a| a.1 == "fx_smoke"), "{asked:?}");
        let smoke_only = still.damage_puffs;
        let (heavy, asked) = at(198);
        assert!(asked.iter().any(|a| a.1 == "fx_smoke"));
        assert!(asked.iter().any(|a| a.1 == "fx_fire"), "{asked:?}");
        assert!(
            heavy.damage_puffs > smoke_only,
            "smoke and fire are more puffs than smoke alone"
        );
        // Over 100 frames of 0.1 s: 4 smoke puffs a second is 40 puffs, 8 + 7 a second 150 (the pool's
        // own generator decides the fractions).
        assert!((28..=54).contains(&smoke_only), "{smoke_only}");
        assert!(
            (120..=185).contains(&heavy.damage_puffs),
            "{}",
            heavy.damage_puffs
        );
        // Every puff of the anchored model started at its anchor, none by the rule.
        assert_eq!(heavy.damage_puffs_by_rule, 0);
        assert_eq!(heavy.damage_puffs_at_anchor, heavy.damage_puffs);
    }

    /// A model without the anchor gets the rule of `damage.ron`: above the middle of the body at 0.6 of
    /// its length (the plant's longer side is 3 cells, 12 m, so 7.2 m up), within 0.15 of its length.
    #[test]
    fn a_model_without_anchors_gets_the_rule_of_damage_ron() {
        let mut p = player();
        run_damage(
            &mut p,
            &damaged_world(&[("power_plant", 3, 3, 100)]),
            &NoAnchors,
            &frame(1.0, 200.0),
            3,
        );
        let c = p.counters();
        assert!(c.damage_puffs > 0);
        assert_eq!(c.damage_puffs_at_anchor, 0);
        assert_eq!(c.damage_puffs_by_rule, c.damage_puffs);
        // The middle of cell (3, 3) is (14, 14) m. Puffs rise a little in the frames shown (smoke
        // 2.6 m/s, fire 2.0 m/s, 0.3 s at most, jitter 1.2); the disc is 1.8 m.
        for [x, y, z] in p.pool.positions() {
            assert!(
                (x - 14.0).abs() <= 1.8 + 0.5 && (z - 14.0).abs() <= 1.8 + 0.5,
                "{x} {z}"
            );
            assert!((7.2..=7.2 + 1.2).contains(&y), "height {y}");
        }
    }

    /// With an anchor the puffs start at it, lifted by `anchor_lift_m` (1 m), in the anchor's disc (0.4 m):
    /// the anchor of the drawn model, not the rule.
    #[test]
    fn puffs_start_at_the_anchor_a_little_above_it() {
        let mut p = player();
        let anchors = fixed(Some([30.0, 6.0, 20.0]));
        // The object is on screen (cell 3, 3 is 14 m from the origin); the anchor is somewhere else.
        run_damage(
            &mut p,
            &damaged_world(&[("power_plant", 3, 3, 100)]),
            &anchors,
            &frame(1.0, 200.0),
            3,
        );
        let c = p.counters();
        assert!(c.damage_puffs > 0);
        assert_eq!(c.damage_puffs_at_anchor, c.damage_puffs);
        assert_eq!(c.damage_puffs_by_rule, 0);
        for [x, y, z] in p.pool.positions() {
            assert!(
                (x - 30.0).abs() <= 0.4 + 0.5 && (z - 20.0).abs() <= 0.4 + 0.5,
                "{x} {z}"
            );
            assert!((7.0..=7.0 + 1.2).contains(&y), "height {y}");
        }
    }

    /// Repaired above the threshold, the object stops at once; an object that is gone from the snapshot
    /// (dead) does not smoke either.
    #[test]
    fn repair_above_the_threshold_or_death_stops_the_effect() {
        let mut p = player();
        let f = frame(1.0, 200.0);
        let hurt = damaged_world(&[("power_plant", 3, 3, 100)]);
        run_damage(&mut p, &hurt, &NoAnchors, &f, 20);
        let made = p.counters().damage_puffs;
        assert!(made > 0);
        // Repaired to 500 of 600 (833 thousandths): not one more puff.
        let repaired = damaged_world(&[("power_plant", 3, 3, 500)]);
        run_damage(&mut p, &repaired, &NoAnchors, &f, 50);
        assert_eq!(p.counters().damage_puffs, made);
        assert_eq!(p.counters().damage_sources, 0);
        // Damaged again, then dead (not in the snapshot): nothing more.
        run_damage(&mut p, &hurt, &NoAnchors, &f, 20);
        let again = p.counters().damage_puffs;
        assert!(again > made);
        run_damage(&mut p, &damaged_world(&[]), &NoAnchors, &f, 50);
        assert_eq!(p.counters().damage_puffs, again);
    }

    /// Units smoke too, by the rule; infantry (under a cell) and objects made without the unit table do not.
    #[test]
    fn a_damaged_vehicle_smokes_but_infantry_does_not() {
        let f = frame(1.0, 200.0);
        let mut p = player();
        // 100 of 400 hit points: heavily damaged.
        let tank = damaged_world(&[("light_tank", 3, 3, 100)]);
        run_damage(&mut p, &tank, &NoAnchors, &f, 50);
        assert!(p.counters().damage_puffs > 0, "a vehicle smokes");
        let mut p = player();
        let rifle = damaged_world(&[("rifleman", 3, 3, 5)]);
        run_damage(&mut p, &rifle, &NoAnchors, &f, 50);
        assert_eq!(p.counters().damage_puffs, 0, "infantry does not");
        // A placeholder (made without the table) has no hit points to speak of.
        let (mut views, table) = damaged_views(&[("light_tank", 3, 3, 100)]);
        views[0].has_stats = false;
        let mut p = player();
        run_damage(
            &mut p,
            &SimSnapshot::of_views(views, table),
            &NoAnchors,
            &f,
            50,
        );
        assert_eq!(p.counters().damage_puffs, 0);
    }

    /// The zoom: nothing from afar (icons only), half the rate in the middle, whole at close range.
    #[test]
    fn damage_effects_follow_the_zoom_like_the_rest() {
        let snap = damaged_world(&[("power_plant", 3, 3, 100)]);
        let puffs = |model_weight: f32, reference_px: f32| {
            let mut p = player();
            let f = frame(model_weight, reference_px);
            run_damage(&mut p, &snap, &NoAnchors, &f, 200);
            p.counters()
        };
        let close = puffs(1.0, 200.0);
        let middle = puffs(1.0, 40.0);
        let far = puffs(0.0, 4.0);
        assert_eq!((far.damage_puffs, far.damage_sources), (0, 0));
        assert!(close.damage_puffs > 0);
        let ratio = middle.damage_puffs as f32 / close.damage_puffs as f32;
        assert!((0.35..=0.65).contains(&ratio), "middle/close = {ratio}");
    }

    /// An object off the screen is skipped without a lookup; and at most `max_sources` give off puffs in a
    /// frame, the rest counted.
    #[test]
    fn off_screen_objects_are_skipped_and_the_sources_are_capped() {
        let f = frame(1.0, 200.0);
        // Cell (200, 200) is far off the 960 x 540 picture of the camera at (12, 12) m.
        let mut p = player();
        let anchors = fixed(Some([800.0, 2.0, 800.0]));
        let far = damaged_world(&[("power_plant", 200, 200, 100)]);
        run_damage(&mut p, &far, &anchors, &f, 20);
        assert_eq!(p.counters().damage_puffs, 0);
        assert!(anchors.asked.borrow().is_empty(), "no lookup off screen");
        // Ten on screen against a cap of 4.
        let capped = Damage::parse(
            &sf_data::damage::EMBEDDED_DAMAGE.replace("max_sources: 256", "max_sources: 4"),
        )
        .unwrap();
        let mut p =
            FxPlayer::with_damage(Effects::embedded(), &UnitDefs::embedded(), capped).unwrap();
        let items: Vec<_> = (0..10)
            .map(|i| ("power_plant", 1 + i % 5, 2 + i / 5, 100))
            .collect();
        run_damage(&mut p, &damaged_world(&items), &NoAnchors, &f, 1);
        let c = p.counters();
        assert_eq!((c.damage_sources, c.damage_sources_capped), (4, 6));
    }

    /// The pool's budget holds with 200 burning objects on screen.
    #[test]
    fn many_burning_objects_stay_inside_the_pool() {
        let items: Vec<_> = (0..200)
            .map(|i| ("power_plant", 1 + i % 20, 1 + i / 20, 50))
            .collect();
        let snap = damaged_world(&items);
        let mut p = player();
        run_damage(&mut p, &snap, &NoAnchors, &frame(1.0, 200.0), 400);
        let stats = p.pool_stats();
        let cap = p.limits().max_particles;
        assert!(stats.alive <= cap && stats.peak <= cap, "{stats:?}");
        assert!(stats.spawned > 0);
        assert!(p.counters().damage_sources <= 200);
    }

    // ---- What the damage effects cost (WP6.8): measurements, run by hand in release ----

    /// 5,000 objects of the fight scale: 4,800 tanks over 240 x 240 m, of which `hurt_tanks` are heavily
    /// damaged, and `burning` power plants (heavily damaged) in a block at the middle.
    fn crowd_of(hurt_tanks: usize, burning: usize) -> SimSnapshot {
        let mut items: Vec<(&str, i32, i32, u32)> = Vec::new();
        for i in 0..4800usize {
            let (x, y) = ((i % 80) as i32 * 3, (i / 80) as i32 * 3);
            // Every `4800 / hurt_tanks`-th tank is heavily damaged (of 400 hit points).
            let hurt = hurt_tanks > 0 && i % (4800 / hurt_tanks) == 0;
            items.push(("light_tank", x, y, if hurt { 50 } else { 400 }));
        }
        for i in 0..burning {
            items.push((
                "power_plant",
                30 + (i % 20) as i32 * 4,
                40 + (i / 20) as i32 * 4,
                50,
            ));
        }
        // Ascending slots are the order given, whatever the cells.
        damaged_world(&items)
    }

    /// A camera 300 m up looking straight down at the middle of that area.
    fn frame_overhead() -> FxFrame {
        let eye = Vec3::new(120.0, 300.0, 121.0);
        let view = sf_render::glam::camera::rh::view::look_at_mat4(
            eye,
            Vec3::new(120.0, 0.0, 120.0),
            Vec3::Y,
        );
        let proj =
            sf_render::glam::camera::rh::proj::directx::perspective(0.8, 16.0 / 9.0, 0.1, 800.0);
        FxFrame {
            view_proj: proj * view,
            focal_px: 0.5 * 540.0 / (0.4_f32).tan(),
            viewport: [960.0, 540.0],
            model_weight: 1.0,
            reference_px: 200.0,
            icon_px: 18.0,
            dt: 0.05,
            alpha: 1.0,
        }
    }

    /// The table with the damage thresholds so low that nothing ever counts as damaged: the same code path
    /// with the feature switched off (the baseline of the measurement).
    fn damage_off() -> Damage {
        Damage::parse(
            &sf_data::damage::EMBEDDED_DAMAGE
                .replace("damaged_at_permille: 660", "damaged_at_permille: 2")
                .replace("heavy_at_permille: 330", "heavy_at_permille: 1"),
        )
        .unwrap()
    }

    #[test]
    #[ignore = "a measurement, run by hand: cargo test --release -p sf_game --lib what_the_damage_effects_cost -- --ignored --nocapture"]
    fn what_the_damage_effects_cost() {
        let ground = FlatGround::default();
        let f = frame_overhead();
        let defs = UnitDefs::embedded();
        let scenes = [
            ("5,000 objects, none damaged", crowd_of(0, 0)),
            ("5,000 + 200 burning buildings", crowd_of(0, 200)),
            ("5,000 + 200 burning + 480 hurt tanks", crowd_of(480, 200)),
        ];
        println!(
            "load average before: {}",
            std::fs::read_to_string("/proc/loadavg")
                .unwrap_or_default()
                .trim()
        );
        for (name, snap) in &scenes {
            let mut rows = Vec::new();
            for round in 0..5 {
                let time = |damage: Damage| {
                    let mut p = FxPlayer::with_damage(Effects::embedded(), &defs, damage).unwrap();
                    for _ in 0..100 {
                        p.frame_with(snap, &ground, &NoAnchors, &f);
                    }
                    let t = std::time::Instant::now();
                    for _ in 0..200 {
                        p.frame_with(snap, &ground, &NoAnchors, &f);
                    }
                    let us = t.elapsed().as_secs_f64() * 1e6 / 200.0;
                    (us, p.counters(), p.pool_stats())
                };
                let (off, _, _) = time(damage_off());
                let (on, c, stats) = time(Damage::embedded());
                rows.push((off, on));
                if round == 4 {
                    println!(
                        "  last round: sources {} (capped {} in all), instances {}, pool alive {} peak {} smoke refused {} dropped {}",
                        c.damage_sources,
                        c.damage_sources_capped,
                        c.instances,
                        stats.alive,
                        stats.peak,
                        stats.smoke_refused,
                        stats.dropped
                    );
                }
            }
            let med = |mut v: Vec<f64>| {
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                v[v.len() / 2]
            };
            println!(
                "{name}: frame_with off {:.1} us, on {:.1} us (medians of 5 alternating rounds; per round off/on: {})",
                med(rows.iter().map(|r| r.0).collect()),
                med(rows.iter().map(|r| r.1).collect()),
                rows.iter()
                    .map(|r| format!("{:.0}/{:.0}", r.0, r.1))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        println!(
            "load average after: {}",
            std::fs::read_to_string("/proc/loadavg")
                .unwrap_or_default()
                .trim()
        );
    }

    /// The software rasterizer's time for the particles of 200 burning buildings in a 960 x 540 frame
    /// (nothing else drawn), against the same frame with no effects: how the draw cost grows, not what a
    /// GPU does.
    #[test]
    #[ignore = "a measurement, run by hand: cargo test --release -p sf_game --lib what_the_damage_particles_cost_to_draw -- --ignored --nocapture"]
    fn what_the_damage_particles_cost_to_draw() {
        let ground = FlatGround::default();
        let defs = UnitDefs::embedded();
        // A closer camera than the CPU measurement: the block of 200 buildings (304 x 144 m) fills the picture.
        let eye = Vec3::new(274.0, 190.0, 310.0);
        let view = sf_render::glam::camera::rh::view::look_at_mat4(
            eye,
            Vec3::new(274.0, 0.0, 232.0),
            Vec3::Y,
        );
        let proj =
            sf_render::glam::camera::rh::proj::directx::perspective(0.8, 16.0 / 9.0, 0.1, 800.0);
        let f = FxFrame {
            view_proj: proj * view,
            ..frame_overhead()
        };
        let snap = crowd_of(0, 200);
        let mut p = FxPlayer::with_damage(Effects::embedded(), &defs, Damage::embedded()).unwrap();
        let mut last = Vec::new();
        for _ in 0..300 {
            last = p.frame_with(&snap, &ground, &NoAnchors, &f).to_vec();
        }
        let mut r = sf_render::Renderer::headless(960, 540, &sf_render::GpuOptions::software())
            .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"));
        println!("adapter: {}", r.adapter_summary());
        r.set_scene(&[]);
        r.set_view_proj(f.view_proj);
        println!(
            "{} instances of {} pool, load average before: {}",
            last.len(),
            p.limits().max_particles,
            std::fs::read_to_string("/proc/loadavg")
                .unwrap_or_default()
                .trim()
        );
        let mut rows = Vec::new();
        for _ in 0..5 {
            let mut time = |instances: &[EffectInstance]| {
                r.set_effects(instances);
                r.render_to_image().unwrap();
                let t = std::time::Instant::now();
                for _ in 0..10 {
                    r.render_to_image().unwrap();
                }
                t.elapsed().as_secs_f64() * 1e3 / 10.0
            };
            let off = time(&[]);
            let on = time(&last);
            rows.push((off, on));
        }
        for (off, on) in &rows {
            println!("  frame without effects {off:.1} ms, with {on:.1} ms");
        }
        println!(
            "load average after: {}",
            std::fs::read_to_string("/proc/loadavg")
                .unwrap_or_default()
                .trim()
        );
    }
}
