// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use crate::function_system::{IntoSystem, System};
use crate::world::World;

pub type SystemFn = fn(&mut World);

struct SystemEntry {
    stage: &'static str,
    system: Box<dyn System>,
}

/// Stage-based system scheduler.
///
/// Systems are grouped into stages. Stages run in the order they were first
/// registered. Within a stage, systems run in registration order.
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
    pub fn add_system<P>(
        &mut self,
        stage: &'static str,
        name: &'static str,
        system: impl IntoSystem<P>,
    ) -> &mut Self {
        if !self.stage_order.contains(&stage) {
            self.stage_order.push(stage);
        }
        self.systems.push(SystemEntry {
            stage,
            system: system.into_system(name),
        });
        self
    }

    /// Run all systems in stage order.
    pub fn run(&mut self, world: &mut World) {
        for &stage in &self.stage_order {
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

    #[derive(Debug)]
    struct Counter(u32);
    impl Component for Counter {}

    fn increment_system(world: &mut World) {
        for (_, counter) in world.query_mut::<Counter>() {
            counter.0 += 1;
        }
    }

    fn double_system(world: &mut World) {
        for (_, counter) in world.query_mut::<Counter>() {
            counter.0 *= 2;
        }
    }

    #[test]
    fn schedule_runs_systems_in_stage_order() {
        let mut world = World::new();
        world.spawn((Counter(1),));

        let mut schedule = Schedule::new();
        // Add increment to "simulate", double to "post".
        schedule.add_system::<()>("simulate", "increment", increment_system as fn(&mut World));
        schedule.add_system::<()>("post", "double", double_system as fn(&mut World));

        schedule.run(&mut world);

        // 1 + 1 = 2, then 2 * 2 = 4
        let val: Vec<u32> = world
            .query::<Counter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(val, vec![4]);
    }

    #[test]
    fn schedule_systems_within_stage_run_in_order() {
        let mut world = World::new();
        world.spawn((Counter(1),));

        let mut schedule = Schedule::new();
        // Both in same stage: increment runs first, then double.
        schedule.add_system::<()>("simulate", "increment", increment_system as fn(&mut World));
        schedule.add_system::<()>("simulate", "double", double_system as fn(&mut World));

        schedule.run(&mut world);

        // 1 + 1 = 2, then 2 * 2 = 4
        let val: Vec<u32> = world
            .query::<Counter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(val, vec![4]);
    }

    #[test]
    fn schedule_stage_order_matters() {
        let mut world = World::new();
        world.spawn((Counter(1),));

        let mut schedule = Schedule::new();
        // Reverse: double first, then increment.
        schedule.add_system::<()>("pre", "double", double_system as fn(&mut World));
        schedule.add_system::<()>("simulate", "increment", increment_system as fn(&mut World));

        schedule.run(&mut world);

        // 1 * 2 = 2, then 2 + 1 = 3
        let val: Vec<u32> = world
            .query::<Counter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(val, vec![3]);
    }

    #[test]
    fn empty_schedule_is_safe() {
        let mut world = World::new();
        let mut schedule = Schedule::new();
        schedule.run(&mut world); // no-op
    }

    use crate::system_param::{QueryMut, Res};

    struct DeltaTime(f64);

    fn param_increment(mut counters: QueryMut<'_, Counter>, dt: Res<'_, DeltaTime>) {
        for (_, c) in counters.iter_mut() {
            c.0 += dt.0 as u32;
        }
    }

    #[test]
    fn schedule_runs_parameterized_system() {
        let mut world = World::new();
        world.spawn((Counter(0),));
        world.insert_resource(DeltaTime(5.0));
        let mut schedule = Schedule::new();
        schedule.add_system::<(QueryMut<'_, Counter>, Res<'_, DeltaTime>)>("simulate", "param_increment", param_increment);
        schedule.run(&mut world);
        let val: Vec<u32> = world.query::<Counter>().into_iter().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![5]);
    }

    #[test]
    fn schedule_mixes_legacy_and_param_systems() {
        let mut world = World::new();
        world.spawn((Counter(0),));
        world.insert_resource(DeltaTime(10.0));
        let mut schedule = Schedule::new();
        schedule.add_system::<()>("pre", "legacy", increment_system as fn(&mut World));
        schedule.add_system::<(QueryMut<'_, Counter>, Res<'_, DeltaTime>)>("simulate", "param", param_increment);
        schedule.run(&mut world);
        let val: Vec<u32> = world.query::<Counter>().into_iter().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![11]);
    }
}
