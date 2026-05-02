// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Particle/billboard primitive.
//!
//! Provides:
//! - [`Emitter`]: a component that spawns [`Particle`] entities each tick at a
//!   configured rate, sampling lifetime / velocity / size / color from
//!   per-emitter [`FloatDist`] / [`Vec3Dist`] / [`ColorDist`] distributions and
//!   capping the alive count at [`Emitter::max`].
//! - [`Particle`]: short-lived entity component with `age`, `lifetime`,
//!   `velocity`, `size`, `color`, and a back-reference to its source emitter.
//! - [`Billboard`]: tag component that opts an entity into the billboard render
//!   path. Rendering is wired separately (T2 of #217, depends on #215).
//! - [`emitter_spawn_expire_system`]: the CPU spawn / expire system. Reads
//!   [`FixedTimestep::step`] for per-tick virtual delta.
//!
//! Distribution sampling is deterministic — each emitter holds a seedable
//! xorshift64 RNG, so identical seeds produce identical particle streams. No
//! external RNG dependency.

use std::collections::HashMap;

use galeon_engine_macros::Component;

use crate::commands::Commands;
use crate::entity::Entity;
use crate::game_loop::FixedTimestep;
use crate::system_param::{QueryMut, Res};

// =============================================================================
// Distributions
// =============================================================================

/// Scalar distribution: constant or uniform `[min, max)`.
///
/// `min > max` is tolerated — bounds are reordered before sampling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatDist {
    /// Always returns the same value.
    Constant(f32),
    /// Uniform `[min, max)`. Equal bounds yield the constant.
    Uniform { min: f32, max: f32 },
}

impl FloatDist {
    /// Draw one sample from this distribution.
    pub fn sample(&self, rng: &mut ParticleRng) -> f32 {
        match *self {
            Self::Constant(v) => v,
            Self::Uniform { min, max } => lerp_uniform(min, max, rng.next_f32()),
        }
    }
}

/// 3D vector distribution: constant or uniform inside an axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Vec3Dist {
    /// Always returns the same vector.
    Constant([f32; 3]),
    /// Uniform inside the AABB defined by `min` / `max`. Per-axis bounds are
    /// reordered if `min[i] > max[i]`.
    UniformBox { min: [f32; 3], max: [f32; 3] },
}

impl Vec3Dist {
    /// Draw one sample from this distribution.
    pub fn sample(&self, rng: &mut ParticleRng) -> [f32; 3] {
        match *self {
            Self::Constant(v) => v,
            Self::UniformBox { min, max } => [
                lerp_uniform(min[0], max[0], rng.next_f32()),
                lerp_uniform(min[1], max[1], rng.next_f32()),
                lerp_uniform(min[2], max[2], rng.next_f32()),
            ],
        }
    }
}

/// RGB color distribution. Channel values are in linear `[0.0, 1.0]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorDist {
    /// Always returns the same color.
    Constant([f32; 3]),
    /// Uniform inside an RGB axis-aligned box.
    UniformBox { min: [f32; 3], max: [f32; 3] },
}

impl ColorDist {
    /// Draw one sample from this distribution.
    pub fn sample(&self, rng: &mut ParticleRng) -> [f32; 3] {
        match *self {
            Self::Constant(v) => v,
            Self::UniformBox { min, max } => [
                lerp_uniform(min[0], max[0], rng.next_f32()),
                lerp_uniform(min[1], max[1], rng.next_f32()),
                lerp_uniform(min[2], max[2], rng.next_f32()),
            ],
        }
    }
}

fn lerp_uniform(a: f32, b: f32, t: f32) -> f32 {
    let lo = a.min(b);
    let hi = a.max(b);
    lo + t * (hi - lo)
}

// =============================================================================
// RNG (xorshift64 with SplitMix-style seed mixing)
// =============================================================================

/// Deterministic per-emitter RNG (xorshift64).
///
/// No external `rand` dependency — keeps the engine crate light and gives
/// downstream callers a stable byte-for-byte particle stream for a given seed.
#[derive(Debug, Clone)]
pub struct ParticleRng {
    state: u64,
}

impl ParticleRng {
    /// Construct an RNG from any `seed`. Zero seeds are remapped (xorshift64
    /// degenerates at zero); identical non-zero seeds yield identical streams.
    pub fn from_seed(seed: u64) -> Self {
        // SplitMix64 mixer: scatters bits even for tiny seeds (0, 1, 2, ...).
        let mut state = seed
            .wrapping_add(0x9E37_79B9_7F4A_7C15)
            .wrapping_mul(0xBF58_476D_1CE4_E5B9);
        state ^= state >> 30;
        state = state.wrapping_mul(0x94D0_49BB_1331_11EB);
        state ^= state >> 27;
        if state == 0 {
            state = 0xDEAD_BEEF_DEAD_BEEF;
        }
        Self { state }
    }

    /// Next raw `u64`. Advances the state by one xorshift round.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Uniform `[0.0, 1.0)` with 24 bits of mantissa precision.
    pub fn next_f32(&mut self) -> f32 {
        let bits = (self.next_u64() >> 40) as u32; // top 24 bits
        bits as f32 / (1u32 << 24) as f32
    }
}

// =============================================================================
// Components
// =============================================================================

/// Particle emitter component.
///
/// Attach to any entity to make it a particle source. Use the builder helpers
/// (`with_velocity`, `with_size`, `with_color`, `with_seed`) to configure
/// distributions before spawning.
///
/// The system that drives emitters is [`emitter_spawn_expire_system`].
#[derive(Component, Debug, Clone)]
pub struct Emitter {
    /// Particles per second.
    pub rate: f32,
    /// Per-particle lifetime distribution (seconds).
    pub lifetime: FloatDist,
    /// Initial velocity distribution (units/second).
    pub velocity: Vec3Dist,
    /// Size distribution (renderer-specific units; consumed by the billboard
    /// render path in T2).
    pub size: FloatDist,
    /// Color distribution (linear RGB `[0, 1]`).
    pub color: ColorDist,
    /// Hard cap on alive particles sourced from this emitter. When reached,
    /// the spawn debt is cleared instead of accumulating.
    pub max: u32,
    /// Fractional spawn debt carried between ticks so non-integer
    /// `rate * step` values still average out to `rate` per second.
    pub spawn_accumulator: f32,
    /// Per-emitter RNG; seedable for deterministic particle streams.
    pub rng: ParticleRng,
}

impl Emitter {
    /// Construct an emitter with sensible defaults: zero velocity, unit size,
    /// white color, RNG seeded at zero.
    pub fn new(rate: f32, lifetime: FloatDist, max: u32) -> Self {
        Self {
            rate,
            lifetime,
            velocity: Vec3Dist::Constant([0.0; 3]),
            size: FloatDist::Constant(1.0),
            color: ColorDist::Constant([1.0, 1.0, 1.0]),
            max,
            spawn_accumulator: 0.0,
            rng: ParticleRng::from_seed(0),
        }
    }

    /// Override the velocity distribution.
    pub fn with_velocity(mut self, velocity: Vec3Dist) -> Self {
        self.velocity = velocity;
        self
    }

    /// Override the size distribution.
    pub fn with_size(mut self, size: FloatDist) -> Self {
        self.size = size;
        self
    }

    /// Override the color distribution.
    pub fn with_color(mut self, color: ColorDist) -> Self {
        self.color = color;
        self
    }

    /// Override the RNG seed for deterministic playback.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.rng = ParticleRng::from_seed(seed);
        self
    }
}

/// Particle component spawned by an [`Emitter`].
///
/// Spawned particles are tagged with [`Billboard`] by the spawn system. The
/// rendering system (T2 of #217) consumes those tags plus the `size`/`color`
/// fields here.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    /// Back-reference to the emitter entity that produced this particle.
    pub source: Entity,
    /// Time elapsed since spawn (seconds). Aged each tick by the system.
    pub age: f32,
    /// Total lifetime (seconds). When `age >= lifetime`, the particle is
    /// despawned.
    pub lifetime: f32,
    /// Initial linear velocity (units/second). Consumed by the renderer or
    /// downstream movement systems.
    pub velocity: [f32; 3],
    /// Particle size (renderer-specific units).
    pub size: f32,
    /// Particle color (linear RGB `[0, 1]`).
    pub color: [f32; 3],
}

/// Tag component opting an entity into the billboard render path.
///
/// The spawn system attaches this tag to every freshly spawned particle so
/// the future T2 rendering path (`#215` instanced billboards) can pick them
/// up via a single component query.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Billboard;

// =============================================================================
// Spawn / expire system
// =============================================================================

/// CPU spawn / expire system for [`Emitter`] / [`Particle`] entities.
///
/// Each tick:
/// 1. Ages every particle by `FixedTimestep::step`. Despawns expired ones.
///    Counts surviving particles per source emitter.
/// 2. For each emitter, spends `rate * step` particles of spawn debt against
///    its remaining cap (`max - alive`). Spawns up to that many new particles
///    with sampled lifetime / velocity / size / color, tagged [`Billboard`].
///
/// Spawn / despawn are deferred via [`Commands`] and applied between schedule
/// stages, so freshly spawned particles will not be aged in the same tick they
/// were spawned.
pub fn emitter_spawn_expire_system(
    ts: Res<'_, FixedTimestep>,
    mut emitters: QueryMut<'_, Emitter>,
    mut particles: QueryMut<'_, Particle>,
    mut cmds: Commands<'_>,
) {
    let step = ts.step as f32;

    // Stage 1: age + expire + count survivors per source.
    let mut alive_per_emitter: HashMap<Entity, u32> = HashMap::new();
    for (entity, particle) in particles.iter_mut() {
        let p: &mut Particle = particle;
        p.age += step;
        if p.age >= p.lifetime {
            cmds.despawn(entity);
        } else {
            *alive_per_emitter.entry(p.source).or_insert(0) += 1;
        }
    }

    // Stage 2: per-emitter spawn budget.
    for (emitter_entity, emitter) in emitters.iter_mut() {
        let e: &mut Emitter = emitter;
        let alive = alive_per_emitter.get(&emitter_entity).copied().unwrap_or(0);
        let headroom = e.max.saturating_sub(alive);
        if headroom == 0 {
            // At cap: drop spawn debt rather than letting a transient burst
            // build up and oversaturate when capacity recovers.
            e.spawn_accumulator = 0.0;
            continue;
        }

        e.spawn_accumulator += e.rate * step;
        if e.spawn_accumulator < 0.0 {
            // Negative rates are nonsensical — clamp the debt back to zero so
            // a misconfigured emitter doesn't silently stay frozen.
            e.spawn_accumulator = 0.0;
            continue;
        }

        let want = e.spawn_accumulator.floor() as u32;
        let n = want.min(headroom);
        e.spawn_accumulator -= n as f32;

        for _ in 0..n {
            let particle = Particle {
                source: emitter_entity,
                age: 0.0,
                lifetime: e.lifetime.sample(&mut e.rng),
                velocity: e.velocity.sample(&mut e.rng),
                size: e.size.sample(&mut e.rng),
                color: e.color.sample(&mut e.rng),
            };
            cmds.spawn((particle, Billboard));
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::game_loop::FixedTimestep;
    use crate::system_param::QueryMut;

    // -- ParticleRng --------------------------------------------------------

    #[test]
    fn rng_zero_seed_does_not_stick() {
        let mut rng = ParticleRng::from_seed(0);
        // 16 draws — none should be all-zero bits.
        for _ in 0..16 {
            assert_ne!(rng.next_u64(), 0);
        }
    }

    #[test]
    fn rng_next_f32_in_unit_interval() {
        let mut rng = ParticleRng::from_seed(42);
        for _ in 0..1024 {
            let v = rng.next_f32();
            assert!((0.0..1.0).contains(&v), "value {v} outside [0,1)");
        }
    }

    #[test]
    fn rng_seed_determinism() {
        let mut a = ParticleRng::from_seed(0xC0FFEE);
        let mut b = ParticleRng::from_seed(0xC0FFEE);
        for _ in 0..32 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn rng_distinct_seeds_diverge() {
        let mut a = ParticleRng::from_seed(1);
        let mut b = ParticleRng::from_seed(2);
        let mut diverged = false;
        for _ in 0..16 {
            if a.next_u64() != b.next_u64() {
                diverged = true;
                break;
            }
        }
        assert!(diverged, "seeds 1 and 2 produced identical streams");
    }

    // -- Distribution sampling ----------------------------------------------

    #[test]
    fn float_constant_returns_value() {
        let mut rng = ParticleRng::from_seed(0);
        let d = FloatDist::Constant(2.5);
        for _ in 0..8 {
            assert_eq!(d.sample(&mut rng), 2.5);
        }
    }

    #[test]
    fn float_uniform_within_bounds() {
        let mut rng = ParticleRng::from_seed(7);
        let d = FloatDist::Uniform { min: 1.0, max: 3.0 };
        for _ in 0..256 {
            let v = d.sample(&mut rng);
            assert!((1.0..3.0).contains(&v), "value {v} outside [1,3)");
        }
    }

    #[test]
    fn float_uniform_swapped_bounds_tolerated() {
        let mut rng = ParticleRng::from_seed(7);
        // min > max: distribution should still produce values in [min, max).
        let d = FloatDist::Uniform { min: 3.0, max: 1.0 };
        for _ in 0..64 {
            let v = d.sample(&mut rng);
            assert!((1.0..3.0).contains(&v), "value {v} outside [1,3)");
        }
    }

    #[test]
    fn vec3_uniform_box_per_axis_bounds() {
        let mut rng = ParticleRng::from_seed(7);
        let d = Vec3Dist::UniformBox {
            min: [-1.0, 0.0, 5.0],
            max: [1.0, 2.0, 7.0],
        };
        for _ in 0..64 {
            let v = d.sample(&mut rng);
            assert!((-1.0..1.0).contains(&v[0]));
            assert!((0.0..2.0).contains(&v[1]));
            assert!((5.0..7.0).contains(&v[2]));
        }
    }

    #[test]
    fn color_constant_returns_value() {
        let mut rng = ParticleRng::from_seed(0);
        let d = ColorDist::Constant([0.25, 0.5, 0.75]);
        assert_eq!(d.sample(&mut rng), [0.25, 0.5, 0.75]);
    }

    // -- Emitter builder ----------------------------------------------------

    #[test]
    fn emitter_new_defaults() {
        let e = Emitter::new(10.0, FloatDist::Constant(1.0), 100);
        assert_eq!(e.rate, 10.0);
        assert_eq!(e.max, 100);
        assert_eq!(e.spawn_accumulator, 0.0);
        assert_eq!(e.velocity, Vec3Dist::Constant([0.0; 3]));
        assert_eq!(e.size, FloatDist::Constant(1.0));
        assert_eq!(e.color, ColorDist::Constant([1.0; 3]));
    }

    #[test]
    fn emitter_builders_are_chainable() {
        let e = Emitter::new(10.0, FloatDist::Constant(1.0), 50)
            .with_velocity(Vec3Dist::Constant([0.0, 1.0, 0.0]))
            .with_size(FloatDist::Constant(0.5))
            .with_color(ColorDist::Constant([1.0, 0.0, 0.0]))
            .with_seed(0xFEED);
        assert_eq!(e.velocity, Vec3Dist::Constant([0.0, 1.0, 0.0]));
        assert_eq!(e.size, FloatDist::Constant(0.5));
        assert_eq!(e.color, ColorDist::Constant([1.0, 0.0, 0.0]));
    }

    // -- emitter_spawn_expire_system ----------------------------------------

    /// Build a 10 Hz engine with the spawn/expire system in stage `simulate`.
    fn engine_with_spawn_system() -> Engine {
        let mut engine = Engine::new();
        engine.set_tick_rate(10.0);
        engine.add_system::<(
            Res<'_, FixedTimestep>,
            QueryMut<'_, Emitter>,
            QueryMut<'_, Particle>,
            Commands<'_>,
        )>(
            "simulate",
            "emitter_spawn_expire",
            emitter_spawn_expire_system,
        );
        engine
    }

    /// Count alive particles in the world.
    fn particle_count(engine: &Engine) -> usize {
        engine.world().query::<&Particle>().count()
    }

    #[test]
    fn no_emitters_no_particles() {
        let mut engine = engine_with_spawn_system();
        engine.tick(1.0);
        assert_eq!(particle_count(&engine), 0);
    }

    #[test]
    fn emitter_at_30hz_spawns_30_per_second() {
        // Issue acceptance: emitter at 30/sec spawns ~30 per simulated second,
        // capped at `max`. 10 Hz tick rate, lifetime > 1s so nothing expires.
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(30.0, FloatDist::Constant(100.0), 1000),));

        for _ in 0..10 {
            engine.tick(0.1);
        }

        assert_eq!(particle_count(&engine), 30);
    }

    #[test]
    fn emitter_respects_max_cap() {
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(30.0, FloatDist::Constant(100.0), 20),));

        for _ in 0..10 {
            engine.tick(0.1);
        }

        // Capped at 20, not 30.
        assert_eq!(particle_count(&engine), 20);
    }

    #[test]
    fn fractional_rate_accumulates_across_ticks() {
        // 15/sec at 10 Hz = 1.5 per tick — the .5 carries via spawn_accumulator.
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(15.0, FloatDist::Constant(100.0), 1000),));

        for _ in 0..10 {
            engine.tick(0.1);
        }

        assert_eq!(particle_count(&engine), 15);
    }

    #[test]
    fn rate_below_one_per_tick_eventually_spawns() {
        // 5/sec at 10 Hz = 0.5 per tick — alternating 0/1 spawn pattern.
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(5.0, FloatDist::Constant(100.0), 1000),));

        for _ in 0..10 {
            engine.tick(0.1);
        }

        assert_eq!(particle_count(&engine), 5);
    }

    #[test]
    fn particles_expire_after_lifetime() {
        // lifetime = 0.05s, step = 0.1s — every particle expires the tick
        // after spawn (because aging happens BEFORE the same-tick spawn).
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(10.0, FloatDist::Constant(0.05), 1000),));

        // Tick 1: spawn 1 (no aging — spawn happens via Commands at end of stage).
        engine.tick(0.1);
        assert_eq!(particle_count(&engine), 1);

        // Tick 2: existing particle ages 0.1s >= 0.05s lifetime → despawned.
        //         Then a fresh particle spawns. Net: still 1.
        engine.tick(0.1);
        assert_eq!(particle_count(&engine), 1);
    }

    #[test]
    fn newly_spawned_particles_visible_after_apply_commands() {
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(10.0, FloatDist::Constant(100.0), 1000),));
        // First tick: 1 spawn (10/sec * 0.1s = 1.0).
        engine.tick(0.1);
        assert_eq!(particle_count(&engine), 1);
    }

    #[test]
    fn spawned_particles_carry_billboard_tag() {
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(10.0, FloatDist::Constant(100.0), 1000),));
        engine.tick(0.1);

        // Every Particle should also have a Billboard tag.
        let particles = engine.world().query::<&Particle>().count();
        let billboards = engine.world().query::<&Billboard>().count();
        assert_eq!(particles, 1);
        assert_eq!(billboards, 1);
    }

    #[test]
    fn paused_engine_does_not_spawn() {
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(30.0, FloatDist::Constant(100.0), 1000),));
        engine.pause();

        for _ in 0..10 {
            engine.tick(0.1);
        }

        assert_eq!(particle_count(&engine), 0);
    }

    #[test]
    fn multiple_emitters_independent_caps() {
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(30.0, FloatDist::Constant(100.0), 5),));
        engine
            .world_mut()
            .spawn((Emitter::new(30.0, FloatDist::Constant(100.0), 7),));

        for _ in 0..10 {
            engine.tick(0.1);
        }

        // 5 + 7 = 12 particles total — each emitter capped independently.
        assert_eq!(particle_count(&engine), 12);
    }

    #[test]
    fn emitter_despawn_strands_particles_until_expire() {
        // After an emitter despawns mid-life, its particles continue ticking
        // and expire normally — they are not orphaned.
        let mut engine = engine_with_spawn_system();
        let emitter =
            engine
                .world_mut()
                .spawn((Emitter::new(10.0, FloatDist::Constant(0.3), 1000),));

        for _ in 0..3 {
            engine.tick(0.1);
        }
        let alive_before = particle_count(&engine);
        assert!(alive_before > 0);

        engine.world_mut().despawn(emitter);

        // No new spawns; existing particles age out by 0.3s.
        for _ in 0..4 {
            engine.tick(0.1);
        }
        assert_eq!(particle_count(&engine), 0);
    }

    #[test]
    fn deterministic_with_same_seed() {
        // Two engines with identically seeded emitters produce the same
        // lifetime/velocity/size/color sequence.
        fn run() -> Vec<(f32, [f32; 3], f32, [f32; 3])> {
            let mut engine = engine_with_spawn_system();
            engine.world_mut().spawn((Emitter::new(
                30.0,
                FloatDist::Uniform { min: 0.5, max: 1.5 },
                1000,
            )
            .with_velocity(Vec3Dist::UniformBox {
                min: [-1.0, 0.0, -1.0],
                max: [1.0, 1.0, 1.0],
            })
            .with_size(FloatDist::Uniform { min: 0.1, max: 0.3 })
            .with_color(ColorDist::UniformBox {
                min: [0.0; 3],
                max: [1.0; 3],
            })
            .with_seed(0xCAFE_BABE),));

            engine.tick(0.5);
            let mut samples: Vec<_> = engine
                .world()
                .query::<&Particle>()
                .map(|(_, p)| (p.lifetime, p.velocity, p.size, p.color))
                .collect();
            // Stable order for comparison.
            samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            samples
        }

        assert_eq!(run(), run());
    }

    #[test]
    fn rate_zero_emitter_never_spawns() {
        let mut engine = engine_with_spawn_system();
        engine
            .world_mut()
            .spawn((Emitter::new(0.0, FloatDist::Constant(100.0), 1000),));

        for _ in 0..100 {
            engine.tick(0.1);
        }
        assert_eq!(particle_count(&engine), 0);
    }

    #[test]
    fn at_cap_drops_accumulator_does_not_burst() {
        // After hitting the cap, lingering spawn_accumulator must not let a
        // burst of N particles spawn the moment headroom returns. We check
        // that spawn_accumulator is reset to 0 once the cap is hit.
        let mut engine = engine_with_spawn_system();
        let emitter = engine
            .world_mut()
            .spawn((Emitter::new(30.0, FloatDist::Constant(100.0), 5),));

        for _ in 0..20 {
            engine.tick(0.1);
        }

        // At cap, accumulator should be zero.
        let acc = engine
            .world()
            .get::<Emitter>(emitter)
            .map(|e| e.spawn_accumulator)
            .expect("emitter alive");
        assert_eq!(acc, 0.0);
    }
}
