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
/// Systems are parameterized functions that declare their data access via
/// [`SystemParam`](crate::system_param::SystemParam).
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
    /// Accepts any function that implements [`IntoSystem`] — parameterized
    /// functions like `fn(Res<T>, QueryMut<U>)`.
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

    /// Run all systems in stage order.
    ///
    /// Execution order each tick:
    /// 1. **Drain deadlines** — fires all overdue deadlines into Events `current`.
    /// 2. **Advance event buffers** — `current` → `previous` (fired deadlines
    ///    become readable), old `previous` cleared.
    /// 3. **Run systems by stage** — `EventReader` sees fired deadlines + any
    ///    events from the previous tick. Commands applied between stages.
    pub fn run(&mut self, world: &mut World) {
        // 1. Drain all overdue deadlines → writes to Events<T> current buffer.
        world.drain_all_deadlines();

        // 2. Capture deadline-fired render events from `current` BEFORE the
        //    swap clears it. Each extractor tracks an offset into current so
        //    it only reads events added since the last flush — no duplicates.
        world.flush_render_events();

        // 3. Advance all event buffers: current → previous, clear current.
        //    Deadline + last-tick events move into `previous`, readable by
        //    EventReader. Extractor offsets auto-reset when current shrinks.
        world.update_events();

        for stage_idx in 0..self.stage_order.len() {
            let stage = self.stage_order[stage_idx];
            for entry in &mut self.systems {
                if entry.stage == stage {
                    entry.system.run(world);
                }
            }
            world.apply_commands();
        }

        // 4. Capture system-written render events from `current`.
        world.flush_render_events();
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

    fn increment_system(mut counters: QueryMut<'_, Counter>) {
        for (_, counter) in counters.iter_mut() {
            counter.0 += 1;
        }
    }

    fn double_system(mut counters: QueryMut<'_, Counter>) {
        for (_, counter) in counters.iter_mut() {
            counter.0 *= 2;
        }
    }

    #[test]
    fn schedule_runs_systems_in_stage_order() {
        let mut world = World::new();
        world.spawn((Counter(1),));

        let mut schedule = Schedule::new();
        schedule.add_system::<(QueryMut<'_, Counter>,)>("simulate", "increment", increment_system);
        schedule.add_system::<(QueryMut<'_, Counter>,)>("post", "double", double_system);

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
        schedule.add_system::<(QueryMut<'_, Counter>,)>("simulate", "increment", increment_system);
        schedule.add_system::<(QueryMut<'_, Counter>,)>("simulate", "double", double_system);

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
        schedule.add_system::<(QueryMut<'_, Counter>,)>("pre", "double", double_system);
        schedule.add_system::<(QueryMut<'_, Counter>,)>("simulate", "increment", increment_system);

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
    fn schedule_multi_param_systems_across_stages() {
        let mut world = World::new();
        world.insert_resource(Speed(10.0));
        world.spawn((Counter(0),));

        let mut schedule = Schedule::new();
        schedule.add_system::<(QueryMut<'_, Counter>,)>("pre", "increment", increment_system);
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

    // -- Commands integration tests --

    use crate::commands::Commands;

    fn spawn_via_commands(mut cmds: Commands<'_>) {
        cmds.spawn((Counter(100),));
    }

    #[test]
    fn schedule_applies_commands_between_stages() {
        let mut world = World::new();

        // Stage "spawn" queues a deferred spawn.
        // Stage "read" should see the spawned entity.
        let mut schedule = Schedule::new();
        schedule.add_system::<(Commands<'_>,)>("spawn", "spawner", spawn_via_commands);
        schedule.add_system::<(QueryMut<'_, Counter>,)>("read", "increment", increment_system);

        schedule.run(&mut world);

        // Entity spawned by commands, then incremented: 100 + 1 = 101
        let vals: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(vals, vec![101]);
    }

    fn despawn_all_via_commands(
        counters: crate::system_param::Query<'_, Counter>,
        mut cmds: Commands<'_>,
    ) {
        for (entity, _) in counters.iter() {
            cmds.despawn(entity);
        }
    }

    #[test]
    fn schedule_commands_despawn_visible_to_next_stage() {
        let mut world = World::new();
        world.spawn((Counter(1),));
        world.spawn((Counter(2),));

        let mut schedule = Schedule::new();
        schedule.add_system::<(crate::system_param::Query<'_, Counter>, Commands<'_>)>(
            "cleanup",
            "despawn_all",
            despawn_all_via_commands,
        );
        schedule.add_system::<(QueryMut<'_, Counter>,)>("post", "increment", increment_system);

        schedule.run(&mut world);

        // All entities despawned between stages — nothing to increment.
        assert_eq!(world.entity_count(), 0);
    }

    // -------------------------------------------------------------------------
    // Events integration test
    // -------------------------------------------------------------------------

    use crate::event::{EventReader, EventWriter};

    #[derive(Debug, PartialEq)]
    struct ScoreEvent {
        points: u32,
    }

    fn produce_event(mut writer: EventWriter<'_, ScoreEvent>) {
        writer.send(ScoreEvent { points: 10 });
    }

    fn consume_event(reader: EventReader<'_, ScoreEvent>, mut counters: QueryMut<'_, Counter>) {
        let total: u32 = reader.read().map(|e| e.points).sum();
        for (_, counter) in counters.iter_mut() {
            counter.0 += total;
        }
    }

    #[test]
    fn schedule_event_writer_reader_cross_tick() {
        let mut world = World::new();
        world.add_event::<ScoreEvent>();
        world.spawn((Counter(0),));

        let mut schedule = Schedule::new();
        // System A writes events in stage "produce".
        schedule.add_system::<(EventWriter<'_, ScoreEvent>,)>("produce", "produce", produce_event);
        // System B reads events in stage "consume".
        schedule.add_system::<(EventReader<'_, ScoreEvent>, QueryMut<'_, Counter>)>(
            "consume",
            "consume",
            consume_event,
        );

        // Run 1: system A sends the event (goes to current buffer).
        // update_events runs at the start, but current is empty — nothing moves.
        schedule.run(&mut world);

        // Counter unchanged: no events were in previous during run 1.
        let val: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![0]);

        // Run 2: update_events moves run-1's current → previous.
        // System B can now read the ScoreEvent(10) and adds 10 to the counter.
        schedule.run(&mut world);

        let val: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![10]);
    }

    #[test]
    fn schedule_events_cleared_after_two_ticks() {
        let mut world = World::new();
        world.add_event::<ScoreEvent>();
        world.spawn((Counter(0),));

        let mut schedule = Schedule::new();
        schedule.add_system::<(EventWriter<'_, ScoreEvent>,)>("produce", "produce", produce_event);
        schedule.add_system::<(EventReader<'_, ScoreEvent>, QueryMut<'_, Counter>)>(
            "consume",
            "consume",
            consume_event,
        );

        // Tick 1: event sent to current.
        schedule.run(&mut world);
        // Tick 2: event moves to previous, reader adds 10.
        schedule.run(&mut world);
        // Tick 3: previous cleared (run 3's update_events), new event sent.
        //         Reader adds 10 again (from run 2's send).
        schedule.run(&mut world);

        // After tick 3 the counter has 10 (tick 2 read) + 10 (tick 3 read) = 20.
        let val: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(val, vec![20]);
    }

    // -------------------------------------------------------------------------
    // Render event accumulation tests
    // -------------------------------------------------------------------------

    use crate::render_event::{RenderEvent, RenderEventRegistry};

    #[derive(Debug)]
    struct ImpactRenderEvent {
        entity_index: u32,
    }

    impl RenderEvent for ImpactRenderEvent {
        const KIND: u32 = 1;
        fn entity(&self) -> u32 {
            self.entity_index
        }
        fn position(&self) -> [f32; 3] {
            [0.0; 3]
        }
    }

    #[test]
    fn schedule_flush_captures_system_written_render_events() {
        let mut world = World::new();
        world.add_event::<ImpactRenderEvent>();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactRenderEvent>();
        world.insert_resource(registry);

        fn emit_impact(mut writer: EventWriter<'_, ImpactRenderEvent>) {
            writer.send(ImpactRenderEvent { entity_index: 42 });
        }

        let mut schedule = Schedule::new();
        schedule.add_system::<(EventWriter<'_, ImpactRenderEvent>,)>("sim", "emit", emit_impact);

        schedule.run(&mut world);

        // Flush happened at end of schedule.run() — drain should have the event.
        let events = world.resource::<RenderEventRegistry>().drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].entity, 42);
    }

    #[test]
    fn schedule_flush_captures_deadline_fired_render_events() {
        use crate::deadline::{Clock, Deadlines, TestClock, Timestamp};

        let mut world = World::new();
        world.add_event::<ImpactRenderEvent>();
        world.add_deadline_type::<ImpactRenderEvent>();

        // Install a clock so drain_all_deadlines actually fires.
        world
            .insert_resource(Box::new(TestClock::new(Timestamp::from_micros(0))) as Box<dyn Clock>);

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactRenderEvent>();
        world.insert_resource(registry);

        // Schedule a deadline at time 0 — clock is at 0, so it fires immediately.
        world
            .resource_mut::<Deadlines<ImpactRenderEvent>>()
            .schedule(
                Timestamp::from_micros(0),
                ImpactRenderEvent { entity_index: 99 },
            );

        let mut schedule = Schedule::new();
        schedule.run(&mut world);

        // Pre-swap flush captured the deadline event before update_events cleared current.
        let events = world.resource::<RenderEventRegistry>().drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].entity, 99);
    }

    #[test]
    fn multi_tick_render_events_accumulate_across_schedule_runs() {
        let mut world = World::new();
        world.add_event::<ImpactRenderEvent>();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactRenderEvent>();
        world.insert_resource(registry);

        fn emit_impact(mut writer: EventWriter<'_, ImpactRenderEvent>) {
            writer.send(ImpactRenderEvent { entity_index: 1 });
        }

        let mut schedule = Schedule::new();
        schedule.add_system::<(EventWriter<'_, ImpactRenderEvent>,)>("sim", "emit", emit_impact);

        // Two ticks without draining — simulates multi-tick frame.
        schedule.run(&mut world);
        schedule.run(&mut world);

        let events = world.resource::<RenderEventRegistry>().drain();
        assert_eq!(events.len(), 2);
    }
}
