//! Particles and tracers (WP5.6a): the pool, the zoom rules and the instance the GPU draws.
//!
//! Presentation only. The game turns its events into [`Burst`]s ([`ParticlePool::spawn`]) and its projectiles
//! into tracer dots ([`tracer_dots`]); every frame [`ParticlePool::update`] moves the particles and
//! [`ParticlePool::emit`] writes the ones in view as [`EffectInstance`]s into a vector that was made once, which
//! [`crate::Renderer::set_effects`] copies into a buffer that was made once: one instanced draw call
//! (`assets/shaders/effect.wgsl`), premultiplied alpha, no per-frame allocation. `docs/rules/effects.md` has the
//! rules; the numbers are `assets/config/effects.ron`.
//!
//! # The pool
//!
//! One ring of at most `limits.max_particles` particles in the order they were made. A burst into a full
//! pool pushes the oldest particles out ([`PoolStats::dropped`]). A smoke-class burst (not `glow`) is refused
//! while the pool is more than `limits.smoke_refuse_percent` full ([`PoolStats::smoke_refused`]), so that
//! flashes and explosions keep their room.
//!
//! # Zoom
//!
//! * Size: a particle is `size` metres across, drawn between `limits.min_px` and `limits.max_px` pixels
//!   ([`size_px`]), the same maths as the icons' reference size.
//! * Fade: the alpha of everything is multiplied by the model weight of the model/icon cross-fade
//!   ([`crate::Blend::model`]), so effects are gone where the units have become icons.
//! * Density: a burst makes `limits.middle_factor` of its particles at middle zoom ([`ZoomBand`]).
//! * The exception: a death makes one *flash* at strategic zoom, which keeps `limits.strategic_flash_alpha`
//!   and is no bigger than an icon, so a battle stays visible from the overview.

use std::collections::VecDeque;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use sf_data::effects::{EffectDef, Limits, Tracer};

use crate::style::srgb_to_linear;

/// One instance on the GPU: 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct EffectInstance {
    /// World position of the centre, metres.
    pub position: [f32; 3],
    /// Side of the quad on the screen, physical pixels (already clamped, [`size_px`]).
    pub size_px: f32,
    /// Premultiplied linear colour: rgb times alpha, and the blend alpha. A glowing particle has blend alpha 0
    /// (the pipeline adds its colour), smoke has its alpha.
    pub color: [f32; 4],
}

/// How close the camera is, for the density and the fade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomBand {
    /// Models up close: full bursts.
    Close,
    /// A smaller picture (or the cross-fade): fewer particles.
    Middle,
    /// Only icons: effects have faded out, a death keeps one flash.
    Strategic,
}

impl ZoomBand {
    /// The band for a model weight (`0` = icons only, `1` = models only) and the apparent size in pixels of
    /// the 2 m reference unit. Strategic exactly when only icons are drawn; middle in the cross-fade and
    /// below `limits.middle_below_px`.
    pub fn of(model_weight: f32, reference_px: f32, limits: &Limits) -> ZoomBand {
        if model_weight <= 0.0 {
            ZoomBand::Strategic
        } else if model_weight < 1.0 || reference_px < limits.middle_below_px {
            ZoomBand::Middle
        } else {
            ZoomBand::Close
        }
    }

    /// How many of `count` particles a burst makes in this band: all, `middle_factor` of them rounded up
    /// (at least one), or none (the strategic flash is a burst of its own).
    pub fn count(self, count: u32, limits: &Limits) -> u32 {
        match self {
            ZoomBand::Close => count,
            ZoomBand::Middle => ((count as f32 * limits.middle_factor).ceil() as u32).max(1),
            ZoomBand::Strategic => 0,
        }
    }
}

/// The size on the screen, in physical pixels, of something `size_m` metres across at clip depth `depth`
/// (the `w` of its clip position) for a camera of focal length `focal_px`: clamped to the limits.
pub fn size_px(size_m: f32, depth: f32, focal_px: f32, limits: &Limits) -> f32 {
    (size_m * focal_px / depth.max(0.01)).clamp(limits.min_px, limits.max_px)
}

/// A request for a burst of particles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    /// Where, world metres (the middle of the burst).
    pub position: [f32; 3],
    /// The particles start within this radius of `position` (horizontally), metres.
    pub radius_m: f32,
    /// How many.
    pub count: u32,
    /// What each one is.
    pub def: BurstDef,
    /// A death flash: the one particle that stays at strategic zoom.
    pub flash: bool,
}

/// What a particle of a burst looks like and does (an [`EffectDef`] without its name).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BurstDef {
    /// Seconds.
    pub life_s: f32,
    /// Metres across at birth.
    pub size_m: f32,
    /// Metres per second.
    pub growth_mps: f32,
    /// sRGB.
    pub colour: (u8, u8, u8),
    /// Peak alpha.
    pub alpha: f32,
    /// Additive light.
    pub glow: bool,
    /// Metres per second up.
    pub rise_mps: f32,
    /// Metres per second sideways, up to.
    pub spread_mps: f32,
}

impl From<&EffectDef> for BurstDef {
    fn from(e: &EffectDef) -> BurstDef {
        BurstDef {
            life_s: e.life_s,
            size_m: e.size_m,
            growth_mps: e.growth_mps,
            colour: e.colour,
            alpha: e.alpha,
            glow: e.glow,
            rise_mps: e.rise_mps,
            spread_mps: e.spread_mps,
        }
    }
}

/// One particle: 48 bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Particle {
    pos: [f32; 3],
    vel: [f32; 3],
    size_m: f32,
    growth: f32,
    age: f32,
    life: f32,
    colour: [u8; 3],
    /// Bit 0: glow. Bit 1: death flash.
    flags: u8,
    alpha: f32,
}

const GLOW: u8 = 1;
const FLASH: u8 = 2;

/// What the pool counted since it was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoolStats {
    /// Particles alive now.
    pub alive: u32,
    /// The most that were alive at once.
    pub peak: u32,
    /// Particles made.
    pub spawned: u64,
    /// Particles pushed out of a full pool before their time.
    pub dropped: u64,
    /// Smoke particles not made because the pool was too full.
    pub smoke_refused: u64,
}

/// The particle pool. See the module documentation.
pub struct ParticlePool {
    particles: VecDeque<Particle>,
    capacity: usize,
    smoke_limit: usize,
    rng: u64,
    stats: PoolStats,
}

impl ParticlePool {
    /// A pool with the budget of `limits` (the buffer is allocated here, once).
    pub fn new(limits: &Limits) -> ParticlePool {
        let capacity = limits.max_particles as usize;
        ParticlePool {
            particles: VecDeque::with_capacity(capacity),
            capacity,
            smoke_limit: capacity * limits.smoke_refuse_percent as usize / 100,
            rng: 0x9E37_79B9_7F4A_7C15,
            stats: PoolStats::default(),
        }
    }

    /// The counters.
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            alive: self.particles.len() as u32,
            ..self.stats
        }
    }

    /// Particles alive.
    pub fn len(&self) -> usize {
        self.particles.len()
    }

    /// Whether no particle is alive.
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// The pool's size.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// A number in `[0, 1)` from the pool's own generator (presentation only: the same events give the same
    /// picture on one machine, nothing depends on it).
    fn next_unit(&mut self) -> f32 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let bits = x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40;
        bits as f32 / (1u64 << 24) as f32
    }

    /// The world positions of the particles alive, oldest first (tests, and anything that wants to know
    /// where the effects are).
    pub fn positions(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        self.particles.iter().map(|p| p.pos)
    }

    /// Whether a thing that happens with probability `p` (per call) happens now: for rates per second
    /// (`p = rate * dt`). Presentation only, from the pool's own generator.
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_unit() < p
    }

    /// Makes the particles of `burst`. A smoke-class burst is refused while the pool is more than the smoke
    /// limit full; a full pool pushes its oldest particles out. Returns how many were made.
    pub fn spawn(&mut self, burst: &Burst) -> u32 {
        if burst.count == 0 {
            return 0;
        }
        let d = &burst.def;
        if !d.glow && self.particles.len() > self.smoke_limit {
            self.stats.smoke_refused += u64::from(burst.count);
            return 0;
        }
        for _ in 0..burst.count {
            if self.particles.len() >= self.capacity {
                self.particles.pop_front();
                self.stats.dropped += 1;
            }
            // A point in a disc and a direction from the same two numbers per axis.
            let angle = self.next_unit() * std::f32::consts::TAU;
            let r = self.next_unit().sqrt() * burst.radius_m;
            let (s, c) = angle.sin_cos();
            let speed = self.next_unit() * d.spread_mps;
            let angle2 = self.next_unit() * std::f32::consts::TAU;
            let (s2, c2) = angle2.sin_cos();
            let lift = 0.8 + 0.4 * self.next_unit();
            self.particles.push_back(Particle {
                pos: [
                    burst.position[0] + c * r,
                    burst.position[1],
                    burst.position[2] + s * r,
                ],
                vel: [c2 * speed, d.rise_mps * lift, s2 * speed],
                size_m: d.size_m,
                growth: d.growth_mps,
                age: 0.0,
                life: d.life_s,
                colour: [d.colour.0, d.colour.1, d.colour.2],
                flags: (u8::from(d.glow) * GLOW) | (u8::from(burst.flash) * FLASH),
                alpha: d.alpha,
            });
            self.stats.spawned += 1;
        }
        self.stats.peak = self.stats.peak.max(self.particles.len() as u32);
        burst.count
    }

    /// Moves every particle on by `dt` seconds and removes the ones whose time is up.
    pub fn update(&mut self, dt: f32) {
        self.particles.retain_mut(|p| {
            p.age += dt;
            p.pos[0] += p.vel[0] * dt;
            p.pos[1] += p.vel[1] * dt;
            p.pos[2] += p.vel[2] * dt;
            p.age < p.life
        });
    }

    /// Writes the instances of the particles in view into `out` (cleared first; no allocation if `out` has
    /// room). Returns how many.
    pub fn emit(&self, view: &EmitView<'_>, out: &mut Vec<EffectInstance>) -> usize {
        out.clear();
        for p in &self.particles {
            let t = (p.age / p.life).clamp(0.0, 1.0);
            let flash = p.flags & FLASH != 0;
            // The alpha over the particle's life: light fades out; smoke comes in fast and goes slowly.
            let curve = if p.flags & GLOW != 0 {
                1.0 - t
            } else {
                (t * 8.0).min(1.0) * (1.0 - t)
            };
            let (alpha, size_cap) = if flash {
                // Held at the strategic alpha for the first half, then it goes (so its peak is the number
                // in the file), and never bigger than an icon.
                let hold = if t < 0.5 { 1.0 } else { 2.0 * (1.0 - t) };
                (
                    view.limits
                        .strategic_flash_alpha
                        .max(p.alpha * view.model_weight)
                        * hold,
                    view.icon_px,
                )
            } else {
                (p.alpha * curve * view.model_weight, view.limits.max_px)
            };
            if alpha <= 0.0 {
                continue;
            }
            let Some(px) = view.size_for(p.pos, p.size_m + p.growth * p.age, size_cap) else {
                continue;
            };
            out.push(instance(p.pos, px.0, p.colour, alpha, p.flags & GLOW != 0));
        }
        out.len()
    }
}

fn instance(
    pos: [f32; 3],
    size_px: f32,
    colour: [u8; 3],
    alpha: f32,
    glow: bool,
) -> EffectInstance {
    let c = colour.map(srgb_to_linear);
    EffectInstance {
        position: pos,
        size_px,
        color: [
            c[0] * alpha,
            c[1] * alpha,
            c[2] * alpha,
            if glow { 0.0 } else { alpha },
        ],
    }
}

/// What a frame knows about the camera and the zoom, for [`ParticlePool::emit`] and [`tracer_dots`].
pub struct EmitView<'a> {
    /// The frame's view-projection.
    pub view_proj: Mat4,
    /// Half the viewport height over `tan(fov_y / 2)` (`EyeView::focal_px`).
    pub focal_px: f32,
    /// The viewport, physical pixels.
    pub viewport: [f32; 2],
    /// The model weight of the cross-fade: `1` models only, `0` icons only.
    pub model_weight: f32,
    /// The side of an icon, physical pixels: what a strategic flash is clamped to.
    pub icon_px: f32,
    /// The budgets and zoom rules.
    pub limits: &'a Limits,
}

impl EmitView<'_> {
    /// The size in pixels of something `size_m` across at `pos`, clamped to the limits and to `cap`; `None`
    /// when it is behind the camera or entirely off the screen.
    fn size_for(&self, pos: [f32; 3], size_m: f32, cap: f32) -> Option<(f32, [f32; 2])> {
        let clip = self.view_proj * Vec3::from(pos).extend(1.0);
        if clip.w <= 0.01 {
            return None;
        }
        let px = size_px(size_m, clip.w, self.focal_px, self.limits)
            .min(cap)
            .max(self.limits.min_px);
        let x = (clip.x / clip.w * 0.5 + 0.5) * self.viewport[0];
        let y = (0.5 - clip.y / clip.w * 0.5) * self.viewport[1];
        let margin = px;
        if x < -margin
            || y < -margin
            || x > self.viewport[0] + margin
            || y > self.viewport[1] + margin
        {
            return None;
        }
        Some((px, [x, y]))
    }
}

/// The dots of one projectile's tracer, head first, written into `out` (appended). `head` is the projectile's
/// world position and `dir` the unit direction it flies in. The dots taper in size and alpha behind the head.
/// Returns how many were written; dots beyond `budget` (the room left in the dot budget) are not.
pub fn tracer_dots(
    tracer: &Tracer,
    head: [f32; 3],
    dir: [f32; 3],
    view: &EmitView<'_>,
    budget: usize,
    out: &mut Vec<EffectInstance>,
) -> usize {
    let mut written = 0;
    for i in 0..tracer.dots as usize {
        if written >= budget {
            break;
        }
        let back = i as f32 * tracer.spacing_m;
        let pos = [
            head[0] - dir[0] * back,
            head[1] - dir[1] * back,
            head[2] - dir[2] * back,
        ];
        let taper = 1.0 - 0.6 * i as f32 / tracer.dots as f32;
        let alpha = tracer.alpha * taper * view.model_weight;
        if alpha <= 0.0 {
            break;
        }
        if let Some((px, _)) = view.size_for(pos, tracer.size_m * taper, view.limits.max_px) {
            out.push(instance(
                pos,
                px,
                [tracer.colour.0, tracer.colour.1, tracer.colour.2],
                alpha,
                true,
            ));
            written += 1;
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_data::effects::Effects;

    fn limits() -> Limits {
        Effects::embedded().limits
    }

    fn glow() -> BurstDef {
        BurstDef {
            life_s: 1.0,
            size_m: 1.0,
            growth_mps: 0.0,
            colour: (255, 255, 255),
            alpha: 1.0,
            glow: true,
            rise_mps: 0.0,
            spread_mps: 0.0,
        }
    }

    fn smoke() -> BurstDef {
        BurstDef {
            glow: false,
            alpha: 0.5,
            ..glow()
        }
    }

    fn burst(count: u32, def: BurstDef) -> Burst {
        Burst {
            position: [0.0, 0.0, 10.0],
            radius_m: 0.0,
            count,
            def,
            flash: false,
        }
    }

    /// A camera at the origin looking down +z: a point at z = 10 is at depth 10.
    fn view(limits: &Limits, model_weight: f32) -> EmitView<'_> {
        let proj = glam::camera::rh::proj::directx::perspective(0.8, 16.0 / 9.0, 0.1, 1000.0)
            * glam::camera::rh::view::look_at_mat4(Vec3::ZERO, Vec3::Z, Vec3::Y);
        let focal = 0.5 * 540.0 / (0.4_f32).tan();
        EmitView {
            view_proj: proj,
            focal_px: focal,
            viewport: [960.0, 540.0],
            model_weight,
            icon_px: 18.0,
            limits,
        }
    }

    #[test]
    fn the_pool_keeps_its_budget_and_pushes_the_oldest_out() {
        let l = limits();
        let mut pool = ParticlePool::new(&l);
        let cap = l.max_particles;
        assert_eq!(cap, 4096);
        // 4096 are made, none dropped.
        for _ in 0..cap {
            pool.spawn(&burst(1, glow()));
        }
        assert_eq!(pool.len(), 4096);
        assert_eq!(pool.stats().dropped, 0);
        // The 4097th pushes the oldest out.
        pool.spawn(&Burst {
            position: [7.0, 0.0, 7.0],
            ..burst(1, glow())
        });
        assert_eq!(pool.len(), 4096, "the count stays at the budget");
        assert_eq!(pool.stats().dropped, 1);
        assert_eq!(pool.stats().peak, 4096);
        assert_eq!(
            pool.particles.back().unwrap().pos[0],
            7.0,
            "the new one is in"
        );
        assert_eq!(pool.particles.front().unwrap().pos[0], 0.0);
        // Many more: still the budget, and one drop each.
        for _ in 0..100 {
            pool.spawn(&burst(1, glow()));
        }
        assert_eq!((pool.len(), pool.stats().dropped), (4096, 101));
    }

    /// The oldest goes: after one more than the budget, the first burst's marker is the first to leave.
    #[test]
    fn the_oldest_particle_is_the_one_that_goes() {
        let l = limits();
        let mut pool = ParticlePool::new(&l);
        for i in 0..l.max_particles {
            pool.spawn(&Burst {
                position: [i as f32, 0.0, 0.0],
                ..burst(1, glow())
            });
        }
        pool.spawn(&Burst {
            position: [-1.0, 0.0, 0.0],
            ..burst(1, glow())
        });
        assert_eq!(
            pool.particles.front().unwrap().pos[0],
            1.0,
            "particle 0 went"
        );
    }

    #[test]
    fn smoke_is_refused_above_three_quarters_and_light_is_not() {
        let l = limits();
        let mut pool = ParticlePool::new(&l);
        let limit = (l.max_particles * 75 / 100) as usize;
        // Fill to exactly the limit with light: smoke is still taken (at the limit, not above it).
        for _ in 0..limit {
            pool.spawn(&burst(1, glow()));
        }
        assert_eq!(pool.len(), 3072);
        assert_eq!(
            pool.spawn(&burst(1, smoke())),
            1,
            "at 75 % smoke is still made"
        );
        assert_eq!(pool.len(), 3073);
        // Above it: refused, counted, and light still goes in.
        assert_eq!(pool.spawn(&burst(5, smoke())), 0);
        assert_eq!(pool.stats().smoke_refused, 5);
        assert_eq!(pool.len(), 3073);
        assert_eq!(pool.spawn(&burst(1, glow())), 1);
        assert_eq!(pool.len(), 3074);
    }

    #[test]
    fn particles_move_grow_and_die() {
        let l = limits();
        let mut pool = ParticlePool::new(&l);
        let def = BurstDef {
            life_s: 1.0,
            rise_mps: 2.0,
            growth_mps: 3.0,
            ..glow()
        };
        pool.spawn(&burst(1, def));
        let y0 = pool.particles[0].pos[1];
        pool.update(0.5);
        assert_eq!(pool.len(), 1);
        let p = pool.particles[0];
        assert!(
            p.pos[1] > y0 + 0.7 && p.pos[1] < y0 + 1.3,
            "rose about 1 m: {}",
            p.pos[1]
        );
        pool.update(0.6);
        assert!(pool.is_empty(), "past its life it is gone");
    }

    #[test]
    fn the_screen_size_is_clamped_between_min_and_max() {
        let l = limits();
        let focal = 1000.0;
        // 1 m at 10 m: 100 px, inside.
        assert_eq!(size_px(1.0, 10.0, focal, &l), 100.0_f32.min(l.max_px));
        // Tiny and far: the least.
        assert_eq!(size_px(0.1, 1000.0, focal, &l), l.min_px);
        // Huge and near: the most.
        assert_eq!(size_px(50.0, 1.0, focal, &l), l.max_px);
        // In between it is the plain ratio.
        let mid = size_px(1.0, 50.0, focal, &l);
        assert!((mid - 20.0).abs() < 1e-4, "{mid}");
        // A zero depth cannot divide by zero.
        assert!(size_px(1.0, 0.0, focal, &l).is_finite());
        assert_eq!((l.min_px, l.max_px), (2.0, 64.0));
    }

    #[test]
    fn the_zoom_bands_and_the_density() {
        let l = limits();
        assert_eq!(ZoomBand::of(1.0, 200.0, &l), ZoomBand::Close);
        assert_eq!(ZoomBand::of(1.0, 59.0, &l), ZoomBand::Middle);
        assert_eq!(ZoomBand::of(1.0, 60.0, &l), ZoomBand::Close);
        assert_eq!(
            ZoomBand::of(0.4, 200.0, &l),
            ZoomBand::Middle,
            "in the cross-fade"
        );
        assert_eq!(ZoomBand::of(0.0, 5.0, &l), ZoomBand::Strategic);
        assert_eq!(ZoomBand::Close.count(8, &l), 8);
        assert_eq!(ZoomBand::Middle.count(8, &l), 4, "half");
        assert_eq!(ZoomBand::Middle.count(5, &l), 3, "rounded up");
        assert_eq!(ZoomBand::Middle.count(1, &l), 1, "never none");
        assert_eq!(ZoomBand::Strategic.count(8, &l), 0);
    }

    #[test]
    fn effects_fade_with_the_model_weight_and_are_gone_at_icons_only() {
        let l = limits();
        let mut pool = ParticlePool::new(&l);
        pool.spawn(&burst(1, glow()));
        pool.spawn(&burst(1, smoke()));
        let mut out = Vec::with_capacity(16);
        // Models only: both drawn at full alpha (smoke at its 0.5 once it has come in).
        pool.update(0.2);
        assert_eq!(pool.emit(&view(&l, 1.0), &mut out), 2);
        let glow_a = out[0].color[0]; // white: rgb = alpha
        pool.emit(&view(&l, 0.5), &mut out);
        assert!(
            (out[0].color[0] - glow_a * 0.5).abs() < 1e-5,
            "half the weight, half the alpha"
        );
        // Icons only: nothing is drawn (alpha 0 at the icon weight).
        assert_eq!(pool.emit(&view(&l, 0.0), &mut out), 0);
    }

    #[test]
    fn a_glowing_particle_has_blend_alpha_zero_and_smoke_keeps_its_alpha() {
        let l = limits();
        let mut pool = ParticlePool::new(&l);
        pool.spawn(&burst(1, glow()));
        pool.spawn(&burst(1, smoke()));
        pool.update(0.2);
        let mut out = Vec::new();
        pool.emit(&view(&l, 1.0), &mut out);
        assert_eq!(out[0].color[3], 0.0, "additive");
        assert!(out[1].color[3] > 0.0, "normal alpha");
        // Premultiplied: the colour is smaller than or equal to the alpha for white smoke.
        assert!((out[1].color[0] - out[1].color[3]).abs() < 1e-5);
    }

    #[test]
    fn a_death_flash_survives_at_strategic_zoom_clamped_to_the_icon() {
        let l = limits();
        let mut pool = ParticlePool::new(&l);
        pool.spawn(&Burst {
            flash: true,
            def: BurstDef {
                size_m: 6.0,
                ..glow()
            },
            ..burst(1, glow())
        });
        pool.spawn(&burst(1, glow()));
        let mut out = Vec::new();
        // Strategic: the ordinary particle is gone, the flash is not.
        let n = pool.emit(&view(&l, 0.0), &mut out);
        assert_eq!(n, 1, "only the flash");
        // Its peak alpha: white glow, rgb = alpha * linear(1.0).
        assert!(
            out[0].color[0] >= 0.5,
            "alpha at least 0.5: {}",
            out[0].color[0]
        );
        assert!((out[0].color[0] - l.strategic_flash_alpha).abs() < 1e-5);
        assert!(
            out[0].size_px <= 18.0,
            "no bigger than an icon: {}",
            out[0].size_px
        );
        // A few frames later it has faded, and is gone at the end of its life.
        pool.update(0.9);
        let n = pool.emit(&view(&l, 0.0), &mut out);
        assert_eq!(n, 1);
        assert!(out[0].color[0] < l.strategic_flash_alpha);
        pool.update(0.2);
        assert_eq!(pool.emit(&view(&l, 0.0), &mut out), 0);
    }

    #[test]
    fn a_particle_behind_the_camera_or_off_screen_is_not_emitted() {
        let l = limits();
        let mut pool = ParticlePool::new(&l);
        pool.spawn(&Burst {
            position: [0.0, 0.0, -5.0],
            ..burst(1, glow())
        });
        pool.spawn(&Burst {
            position: [500.0, 0.0, 10.0],
            ..burst(1, glow())
        });
        let mut out = Vec::new();
        assert_eq!(pool.emit(&view(&l, 1.0), &mut out), 0);
    }

    #[test]
    fn emitting_does_not_grow_the_vector() {
        let l = limits();
        let mut pool = ParticlePool::new(&l);
        for _ in 0..l.max_particles {
            pool.spawn(&burst(1, glow()));
        }
        let mut out: Vec<EffectInstance> =
            Vec::with_capacity((l.max_particles + l.max_tracer_dots) as usize);
        let cap = out.capacity();
        for _ in 0..5 {
            pool.emit(&view(&l, 1.0), &mut out);
        }
        assert_eq!(out.capacity(), cap, "no reallocation");
        assert_eq!(out.len(), 4096);
        assert_eq!(std::mem::size_of::<EffectInstance>(), 32);
        assert_eq!(std::mem::size_of::<Particle>(), 48);
    }

    #[test]
    fn a_tracer_is_a_row_of_tapering_dots_behind_the_head() {
        let e = Effects::embedded();
        let l = &e.limits;
        let v = view(l, 1.0);
        let mut out = Vec::new();
        let n = tracer_dots(
            &e.tracers.straight,
            [0.0, 0.0, 10.0],
            [0.0, 0.0, 1.0],
            &v,
            100,
            &mut out,
        );
        assert_eq!(n, 4);
        assert_eq!(out.len(), 4);
        // Behind the head along -z, spaced 0.9 m, smaller and fainter further back.
        assert!((out[1].position[2] - (10.0 - 0.9)).abs() < 1e-5);
        assert!(out[3].size_px <= out[0].size_px);
        assert!(out[3].color[0] < out[0].color[0]);
        // A budget of two writes two; none, none.
        out.clear();
        assert_eq!(
            tracer_dots(
                &e.tracers.straight,
                [0.0, 0.0, 10.0],
                [0.0, 0.0, 1.0],
                &v,
                2,
                &mut out
            ),
            2
        );
        out.clear();
        assert_eq!(
            tracer_dots(
                &e.tracers.straight,
                [0.0, 0.0, 10.0],
                [0.0, 0.0, 1.0],
                &v,
                0,
                &mut out
            ),
            0
        );
        // Gone at icons only.
        out.clear();
        let strategic = view(l, 0.0);
        assert_eq!(
            tracer_dots(
                &e.tracers.straight,
                [0.0, 0.0, 10.0],
                [0.0, 0.0, 1.0],
                &strategic,
                100,
                &mut out
            ),
            0
        );
    }
}
