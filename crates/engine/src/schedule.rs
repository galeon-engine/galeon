// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use crate::function_system::{IntoSystem, System};
use crate::world::World;

struct SystemEntry {
    stage: &'static str,
    system: Box<dyn System>,
}

/// Stage-based system scheduler.
///
/// Systems are grouped into stages. Stages run in the order they were first
/// registered. Within a stage, systems run in registration order.
///
/// Systems can be either legacy `fn(&mut World)` or parameterized functions
/// that declare their data access via [`SystemParam`](crate::system_param::SystemParam).
pub struct Schedule {
    systems: Vec<SystemEntry>,
    stage_order: Vec<&'static str>,
}

impl Schedule {
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
            stage_order: Vec::new(),
        }
    }

    /// Add a system to a named stage.
    ///
    /// Accepts any function that implements [`IntoSystem`]: both legacy
    /// `fn(&mut World)` (pass as `my_fn as fn(&mut World)`) and
    /// parameterized functions like `fn(Res<T>, QueryMut<U>)`.
    pub fn add_system<P>(
        &mut self,
        stage: &'static str,
        name: &'static str,
        func: impl IntoSystem<P>,
    ) -> &mut Self {
        if !self.stage_order.contains(&stage) {
            self.stage_order.push(stage);
        }
        self.systems.push(SystemEntry {
            stage,
            system: func.into_system(name),
        });
        self
    }

    /// Add a legacy `fn(&mut World)` system without requiring turbofish or cast.
    ///
    /// This is a convenience wrapper for the common case where a system takes
    /// `&mut World` directly. Equivalent to:
    /// ```ignore
    /// schedule.add_system::<()>("stage", "name", my_fn as fn(&mut World));
    /// ```
    pub fn add_legacy_system(
        &mut self,
        stage: &'static str,
        name: &'static str,
        func: fn(&mut World),
    ) -> &mut Self {
        self.add_system::<()>(stage, name, func)
    }

    /// Run all systems in stage order.
    pub fn run(&mut self, world: &mut World) {
        for stage_idx in 0..self.stage_order.len() {
            let stage = self.stage_order[stage_idx];
            for entry in &mut self.systems {
                if entry.stage == stage {
                    entry.system.run(world);
                }
            }
        }
    }

    /// Returns the number of registered systems.
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// Returns the stage names in execution order.
    pub fn stages(&self) -> &[&'static str] {
        &self.stage_order
    }
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::system_param::{QueryMut, Res, ResMut};

    #[derive(Debug)]
    struct Counter(u32);
    impl Component for Counter {}

    // Legacy system (fn(&mut World)):
    fn increment_system(world: &mut World) {
        for (_, counter) in world.query_mut::<&mut Counter>() {
            counter.0 += 1;
        }
    }

    fn double_system(world: &mut World) {
        for (_, counter) in world.query_mut::<&mut Counter>() {
            counter.0 *= 2;
        }
    }

    #[test]
    fn schedule_runs_systems_in_stage_order() {
        let mut world = World::new();
        world.spawn((Counter(1),));

        let mut schedule = Schedule::new();
        schedule.add_system::<()>("simulate", "increment", increment_system as fn(&mut World));
        schedule.add_system::<()>("post", "double", double_system as fn(&mut World));

        schedule.run(&mut world);

        // 1 + 1 = 2, then 2 * 2 = 4
        let val: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![4]);
    }

    #[test]
    fn schedule_systems_within_stage_run_in_order() {
        let mut world = World::new();
        world.spawn((Counter(1),));

        let mut schedule = Schedule::new();
        schedule.add_system::<()>("simulate", "increment", increment_system as fn(&mut World));
        schedule.add_system::<()>("simulate", "double", double_system as fn(&mut World));

        schedule.run(&mut world);

        // 1 + 1 = 2, then 2 * 2 = 4
        let val: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![4]);
    }

    #[test]
    fn schedule_stage_order_matters() {
        let mut world = World::new();
        world.spawn((Counter(1),));

        let mut schedule = Schedule::new();
        schedule.add_system::<()>("pre", "double", double_system as fn(&mut World));
        schedule.add_system::<()>("simulate", "increment", increment_system as fn(&mut World));

        schedule.run(&mut world);

        // 1 * 2 = 2, then 2 + 1 = 3
        let val: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![3]);
    }

    #[test]
    fn empty_schedule_is_safe() {
        let mut world = World::new();
        let mut schedule = Schedule::new();
        schedule.run(&mut world); // no-op
    }

    // -- Parameterized system tests in the schedule --

    fn param_increment(mut counters: QueryMut<'_, Counter>) {
        for (_, c) in counters.iter_mut() {
            c.0 += 1;
        }
    }

    #[test]
    fn schedule_accepts_parameterized_system() {
        let mut world = World::new();
        world.spawn((Counter(0),));

        let mut schedule = Schedule::new();
        schedule.add_system::<(QueryMut<'_, Counter>,)>(
            "update",
            "param_increment",
            param_increment,
        );

        schedule.run(&mut world);

        let val: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![1]);
    }

    struct Speed(f32);

    fn apply_speed(speed: Res<'_, Speed>, mut counters: QueryMut<'_, Counter>) {
        for (_, c) in counters.iter_mut() {
            c.0 += speed.0 as u32;
        }
    }

    #[test]
    fn schedule_mixed_legacy_and_parameterized() {
        let mut world = World::new();
        world.insert_resource(Speed(10.0));
        world.spawn((Counter(0),));

        let mut schedule = Schedule::new();
        // Legacy system first
        schedule.add_system::<()>("pre", "legacy_inc", increment_system as fn(&mut World));
        // Parameterized system second
        schedule.add_system::<(Res<'_, Speed>, QueryMut<'_, Counter>)>(
            "post",
            "apply_speed",
            apply_speed,
        );

        schedule.run(&mut world);

        // 0 + 1 = 1, then 1 + 10 = 11
        let val: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![11]);
    }

    fn increment_speed(mut speed: ResMut<'_, Speed>) {
        speed.0 += 1.0;
    }

    #[test]
    fn schedule_res_mut_persists_across_runs() {
        let mut world = World::new();
        world.insert_resource(Speed(0.0));

        let mut schedule = Schedule::new();
        schedule.add_system::<(ResMut<'_, Speed>,)>("update", "inc_speed", increment_speed);

        schedule.run(&mut world);
        schedule.run(&mut world);

        assert!((world.resource::<Speed>().0 - 2.0).abs() < f32::EPSILON);
    }

    // -- add_legacy_system convenience wrapper (#56) --

    #[test]
    fn add_legacy_system_registers_and_runs() {
        let mut world = World::new();
        world.spawn((Counter(0),));

        let mut schedule = Schedule::new();
        schedule.add_legacy_system("update", "increment", increment_system);

        schedule.run(&mut world);

        let val: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![1]);
    }

    #[test]
    fn add_legacy_system_is_chainable() {
        let mut schedule = Schedule::new();
        schedule
            .add_legacy_system("pre", "increment", increment_system)
            .add_legacy_system("post", "double", double_system);
        assert_eq!(schedule.system_count(), 2);
    }
}
