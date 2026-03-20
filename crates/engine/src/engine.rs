// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use crate::function_system::IntoSystem;
use crate::game_loop::{self, FixedTimestep};
use crate::schedule::Schedule;
use crate::virtual_time::VirtualTime;
use crate::world::World;

/// The central game engine object.
///
/// `Engine` owns the [`World`] and [`Schedule`] and exposes a builder API for
/// wiring up systems and plugins before (or during) the game loop.
///
/// # Example
///
/// ```rust
/// use galeon_engine::{Engine, Plugin};
///
/// fn my_system(_world: &mut galeon_engine::World) {}
///
/// struct MyPlugin;
/// impl Plugin for MyPlugin {
///     fn build(&self, engine: &mut Engine) {
///         engine.add_system::<()>("update", "my_system", my_system as fn(&mut galeon_engine::World));
///     }
/// }
///
/// let mut engine = Engine::new();
/// engine.add_plugin(MyPlugin);
/// engine.run_once();
/// ```
pub struct Engine {
    world: World,
    schedule: Schedule,
}

impl Engine {
    /// Create a new engine with an empty [`World`] and [`Schedule`].
    pub fn new() -> Self {
        Self {
            world: World::new(),
            schedule: Schedule::new(),
        }
    }

    // -------------------------------------------------------------------------
    // Accessors
    // -------------------------------------------------------------------------

    /// Immutable reference to the world.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Mutable reference to the world.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Immutable reference to the schedule.
    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    // -------------------------------------------------------------------------
    // Builder API
    // -------------------------------------------------------------------------

    /// Add a system to the schedule.
    ///
    /// Delegates to [`Schedule::add_system`]. Returns `&mut Self` for chaining.
    pub fn add_system<P>(
        &mut self,
        stage: &'static str,
        name: &'static str,
        system: impl IntoSystem<P>,
    ) -> &mut Self {
        self.schedule.add_system(stage, name, system);
        self
    }

    /// Apply a plugin to this engine.
    ///
    /// Calls [`Plugin::build`] with `self`. Returns `&mut Self` for chaining.
    pub fn add_plugin(&mut self, plugin: impl Plugin) -> &mut Self {
        plugin.build(self);
        self
    }

    /// Insert a resource into the world.
    ///
    /// Delegates to [`World::insert_resource`]. Returns `&mut Self` for
    /// chaining.
    pub fn insert_resource<T: 'static>(&mut self, value: T) -> &mut Self {
        self.world.insert_resource(value);
        self
    }

    // -------------------------------------------------------------------------
    // Execution
    // -------------------------------------------------------------------------

    /// Advance the simulation by `elapsed` seconds using a fixed timestep.
    ///
    /// If a [`FixedTimestep`] resource has not been inserted yet this method
    /// inserts the default RTS timestep (10 Hz) automatically. Returns the
    /// number of ticks executed.
    pub fn tick(&mut self, elapsed: f64) -> u32 {
        // Lazily insert the default timestep so callers don't have to.
        if !self.has_timestep() {
            self.world.insert_resource(FixedTimestep::default_rts());
        }
        game_loop::tick(&mut self.world, &mut self.schedule, elapsed)
    }

    /// Run the schedule exactly once without any fixed-timestep logic.
    ///
    /// Useful for integration tests or non-game-loop scenarios.
    pub fn run_once(&mut self) {
        self.schedule.run(&mut self.world);
    }

    // -------------------------------------------------------------------------
    // Virtual time controls
    // -------------------------------------------------------------------------

    /// Pause the simulation. Ticks will produce zero simulation steps.
    ///
    /// Lazily inserts a default `VirtualTime` if not already present.
    pub fn pause(&mut self) {
        self.ensure_virtual_time();
        self.world.resource_mut::<VirtualTime>().paused = true;
    }

    /// Resume the simulation after a pause.
    ///
    /// Lazily inserts a default `VirtualTime` if not already present.
    pub fn resume(&mut self) {
        self.ensure_virtual_time();
        self.world.resource_mut::<VirtualTime>().paused = false;
    }

    /// Set the simulation speed multiplier (clamped to `[0.0, 8.0]` at tick time).
    ///
    /// - 1.0 = normal speed
    /// - 2.0 = double speed (RTS fast-forward)
    /// - 0.5 = half speed (slow-mo)
    ///
    /// Lazily inserts a default `VirtualTime` if not already present.
    pub fn set_speed(&mut self, scale: f64) {
        self.ensure_virtual_time();
        self.world.resource_mut::<VirtualTime>().scale = scale;
    }

    /// Returns `true` if the simulation is paused.
    pub fn is_paused(&self) -> bool {
        self.world
            .try_resource::<VirtualTime>()
            .is_some_and(|vt| vt.paused)
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Returns `true` if a [`FixedTimestep`] resource is already present.
    fn has_timestep(&self) -> bool {
        // We use a try-pattern by checking via a raw resource probe.
        // `World::resource` panics, so we rely on the resource module's
        // internal try_get once it is available. For now we track it via a
        // small sentinel resource.
        self.world.try_resource::<FixedTimestep>().is_some()
    }

    /// Ensures a `VirtualTime` resource exists, inserting a default if absent.
    fn ensure_virtual_time(&mut self) {
        if self.world.try_resource::<VirtualTime>().is_none() {
            self.world.insert_resource(VirtualTime::new());
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Plugin trait
// =============================================================================

/// A plugin encapsulates a cohesive set of systems and resources.
///
/// Implement this trait to bundle engine configuration into a reusable unit.
///
/// # Example
///
/// ```rust
/// use galeon_engine::{Engine, Plugin, World};
///
/// fn physics_system(_world: &mut World) {}
///
/// pub struct PhysicsPlugin;
///
/// impl Plugin for PhysicsPlugin {
///     fn build(&self, engine: &mut Engine) {
///         engine.add_system::<()>("simulate", "physics", physics_system as fn(&mut World));
///     }
/// }
/// ```
pub trait Plugin {
    /// Configure `engine` with this plugin's systems and resources.
    fn build(&self, engine: &mut Engine);
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;

    #[derive(Debug)]
    struct Counter(u32);
    impl Component for Counter {}

    fn increment(world: &mut World) {
        for (_, c) in world.query_mut::<Counter>() {
            c.0 += 1;
        }
    }

    // -------------------------------------------------------------------------
    // Engine::new / accessors
    // -------------------------------------------------------------------------

    #[test]
    fn new_engine_has_empty_world_and_schedule() {
        let engine = Engine::new();
        assert_eq!(engine.world().entity_count(), 0);
        assert_eq!(engine.schedule().system_count(), 0);
    }

    #[test]
    fn world_mut_allows_mutation() {
        let mut engine = Engine::new();
        engine.world_mut().spawn((Counter(0),));
        assert_eq!(engine.world().entity_count(), 1);
    }

    // -------------------------------------------------------------------------
    // Builder API
    // -------------------------------------------------------------------------

    #[test]
    fn add_system_registers_system() {
        let mut engine = Engine::new();
        engine.add_system::<()>("update", "increment", increment as fn(&mut World));
        assert_eq!(engine.schedule().system_count(), 1);
    }

    #[test]
    fn add_system_is_chainable() {
        let mut engine = Engine::new();
        engine
            .add_system::<()>("pre", "increment", increment as fn(&mut World))
            .add_system::<()>("post", "increment", increment as fn(&mut World));
        assert_eq!(engine.schedule().system_count(), 2);
    }

    #[test]
    fn insert_resource_is_chainable() {
        struct Gravity(f32);

        let mut engine = Engine::new();
        engine.insert_resource(Gravity(9.8));
        assert!((engine.world().resource::<Gravity>().0 - 9.8).abs() < f32::EPSILON);
    }

    // -------------------------------------------------------------------------
    // Plugin
    // -------------------------------------------------------------------------

    struct IncrementPlugin;
    impl Plugin for IncrementPlugin {
        fn build(&self, engine: &mut Engine) {
            engine.add_system::<()>("update", "increment", increment as fn(&mut World));
        }
    }

    #[test]
    fn add_plugin_calls_build() {
        let mut engine = Engine::new();
        engine.add_plugin(IncrementPlugin);
        assert_eq!(engine.schedule().system_count(), 1);
    }

    #[test]
    fn add_plugin_is_chainable() {
        let mut engine = Engine::new();
        engine
            .add_plugin(IncrementPlugin)
            .add_plugin(IncrementPlugin);
        assert_eq!(engine.schedule().system_count(), 2);
    }

    // -------------------------------------------------------------------------
    // run_once
    // -------------------------------------------------------------------------

    #[test]
    fn run_once_executes_schedule() {
        let mut engine = Engine::new();
        engine.world_mut().spawn((Counter(0),));
        engine.add_system::<()>("update", "increment", increment as fn(&mut World));
        engine.run_once();

        let counts: Vec<u32> = engine
            .world()
            .query::<Counter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(counts, vec![1]);
    }

    // -------------------------------------------------------------------------
    // tick
    // -------------------------------------------------------------------------

    #[test]
    fn tick_inserts_default_timestep_when_absent() {
        let mut engine = Engine::new();
        // No FixedTimestep inserted â€” tick should not panic.
        let ticks = engine.tick(0.05); // 0.05 s < 0.1 s step â†’ 0 ticks
        assert_eq!(ticks, 0);
    }

    #[test]
    fn tick_respects_existing_timestep() {
        let mut engine = Engine::new();
        // Use 10 Hz (0.1 s/tick) to avoid floating-point accumulation issues.
        engine.world_mut().insert_resource(FixedTimestep::new(10.0));
        engine.world_mut().spawn((Counter(0),));
        engine.add_system::<()>("update", "increment", increment as fn(&mut World));

        // 0.35 s at 10 Hz â†’ 3 ticks (same as game_loop test)
        let ticks = engine.tick(0.35);
        assert_eq!(ticks, 3);

        let counts: Vec<u32> = engine
            .world()
            .query::<Counter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(counts, vec![3]);
    }

    #[test]
    fn tick_returns_correct_tick_count() {
        let mut engine = Engine::new();
        // Default 10 Hz â†’ 0.25 s yields 2 ticks
        let ticks = engine.tick(0.25);
        assert_eq!(ticks, 2);
    }

    // -------------------------------------------------------------------------
    // Virtual time convenience API
    // -------------------------------------------------------------------------

    #[test]
    fn pause_and_resume() {
        let mut engine = Engine::new();
        assert!(!engine.is_paused());

        engine.pause();
        assert!(engine.is_paused());

        engine.resume();
        assert!(!engine.is_paused());
    }

    #[test]
    fn pause_stops_ticks() {
        let mut engine = Engine::new();
        engine.world_mut().spawn((Counter(0),));
        engine.add_system::<()>("update", "increment", increment as fn(&mut World));

        engine.pause();
        engine.tick(1.0);

        let counts: Vec<u32> = engine
            .world()
            .query::<Counter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(counts, vec![0]);
    }

    #[test]
    fn set_speed_doubles_ticks() {
        let mut engine = Engine::new();
        engine.world_mut().spawn((Counter(0),));
        engine.add_system::<()>("update", "increment", increment as fn(&mut World));

        engine.set_speed(2.0);
        // 0.1s real at 2x = 0.2s virtual, default 10 Hz = 2 ticks
        let ticks = engine.tick(0.1);
        assert_eq!(ticks, 2);
    }

    #[test]
    fn set_speed_persists() {
        let mut engine = Engine::new();
        engine.set_speed(4.0);
        let vt = engine.world().resource::<VirtualTime>();
        assert!((vt.scale - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn lazy_insert_virtual_time() {
        let mut engine = Engine::new();
        assert!(engine.world().try_resource::<VirtualTime>().is_none());

        engine.pause();
        assert!(engine.world().try_resource::<VirtualTime>().is_some());
    }

    use crate::system_param::{QueryMut, Res};

    struct Gravity(f32);

    fn param_system(mut counters: QueryMut<'_, Counter>, _gravity: Res<'_, Gravity>) {
        for (_, c) in counters.iter_mut() {
            c.0 += 1;
        }
    }

    #[test]
    fn engine_accepts_parameterized_system() {
        let mut engine = Engine::new();
        engine.insert_resource(Gravity(9.8));
        engine.world_mut().spawn((Counter(0),));
        engine.add_system::<(QueryMut<'_, Counter>, Res<'_, Gravity>)>(
            "update",
            "param",
            param_system,
        );
        engine.run_once();
        let counts: Vec<u32> = engine
            .world()
            .query::<Counter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(counts, vec![1]);
    }
}
