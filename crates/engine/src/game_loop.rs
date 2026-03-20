// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use crate::schedule::Schedule;
use crate::virtual_time::VirtualTime;
use crate::world::World;

/// Fixed-timestep configuration stored as a resource.
///
/// The game loop ticks the schedule at a fixed rate (default 10 Hz for RTS).
/// A time accumulator ensures deterministic simulation: the same inputs produce
/// the same outputs regardless of frame rate.
pub struct FixedTimestep {
    /// Seconds per tick (1.0 / tick_rate).
    pub step: f64,
    /// Accumulated time not yet consumed by ticks.
    accumulator: f64,
    /// Total number of ticks executed.
    pub tick_count: u64,
}

impl FixedTimestep {
    /// Create a new fixed timestep at the given tick rate (Hz).
    pub fn new(tick_rate: f64) -> Self {
        assert!(tick_rate > 0.0, "tick rate must be positive");
        Self {
            step: 1.0 / tick_rate,
            accumulator: 0.0,
            tick_count: 0,
        }
    }

    /// Create a 10 Hz timestep (default for RTS).
    pub fn default_rts() -> Self {
        Self::new(10.0)
    }

    /// Returns the tick rate in Hz.
    pub fn tick_rate(&self) -> f64 {
        1.0 / self.step
    }
}

/// Advance the simulation by `elapsed` seconds.
///
/// Accumulates time and runs the schedule once per fixed step. Returns the
/// number of ticks executed this frame.
///
/// The `FixedTimestep` must be inserted as a resource on the world before
/// calling this function.
pub fn tick(world: &mut World, schedule: &Schedule, elapsed: f64) -> u32 {
    // Compute virtual elapsed (pass-through if no VirtualTime resource).
    let virtual_elapsed = if let Some(mut vt) = world.try_take_resource::<VirtualTime>() {
        let ve = vt.effective_elapsed(elapsed);
        vt.elapsed += ve;
        world.insert_resource(vt);
        ve
    } else {
        elapsed
    };

    // Remove the timestep resource temporarily to avoid borrow conflicts.
    let mut ts = world.take_resource::<FixedTimestep>();
    ts.accumulator += virtual_elapsed;

    let mut ticks = 0u32;
    while ts.accumulator >= ts.step {
        ts.accumulator -= ts.step;
        ts.tick_count += 1;
        ticks += 1;

        // Re-insert timestep so systems can read it during this tick.
        world.insert_resource(FixedTimestep {
            step: ts.step,
            accumulator: ts.accumulator,
            tick_count: ts.tick_count,
        });
        schedule.run(world);
        // Take it back for the next iteration.
        ts = world.take_resource::<FixedTimestep>();
    }

    // Put the timestep back with remaining accumulator.
    world.insert_resource(ts);
    ticks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::virtual_time::VirtualTime;

    #[derive(Debug)]
    struct TickCounter(u32);
    impl Component for TickCounter {}

    fn count_system(world: &mut World) {
        for (_, counter) in world.query_mut::<TickCounter>() {
            counter.0 += 1;
        }
    }

    #[test]
    fn fixed_timestep_creation() {
        let ts = FixedTimestep::new(10.0);
        assert!((ts.step - 0.1).abs() < f64::EPSILON);
        assert_eq!(ts.tick_count, 0);
        assert!((ts.tick_rate() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tick_runs_correct_number_of_times() {
        let mut world = World::new();
        world.insert_resource(FixedTimestep::new(10.0));
        world.spawn((TickCounter(0),));

        let mut schedule = Schedule::new();
        schedule.add_system("simulate", "count", count_system);

        // 0.25 seconds at 10 Hz = 2 ticks (0.05s remainder)
        let ticks = tick(&mut world, &schedule, 0.25);
        assert_eq!(ticks, 2);

        let counts: Vec<u32> = world
            .query::<TickCounter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(counts, vec![2]);
    }

    #[test]
    fn accumulator_carries_remainder() {
        let mut world = World::new();
        world.insert_resource(FixedTimestep::new(10.0));
        world.spawn((TickCounter(0),));

        let mut schedule = Schedule::new();
        schedule.add_system("simulate", "count", count_system);

        // 0.05s — not enough for a tick
        let ticks = tick(&mut world, &schedule, 0.05);
        assert_eq!(ticks, 0);

        // Another 0.06s — total 0.11s, enough for 1 tick (0.01s remainder)
        let ticks = tick(&mut world, &schedule, 0.06);
        assert_eq!(ticks, 1);

        let counts: Vec<u32> = world
            .query::<TickCounter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(counts, vec![1]);
    }

    #[test]
    fn tick_count_increments() {
        let mut world = World::new();
        world.insert_resource(FixedTimestep::new(10.0));

        let schedule = Schedule::new();

        tick(&mut world, &schedule, 0.35); // 3 ticks
        let ts = world.resource::<FixedTimestep>();
        assert_eq!(ts.tick_count, 3);
    }

    #[test]
    fn systems_can_read_timestep() {
        fn read_step(world: &mut World) {
            let ts = world.resource::<FixedTimestep>();
            assert!((ts.step - 0.1).abs() < f64::EPSILON);
        }

        let mut world = World::new();
        world.insert_resource(FixedTimestep::new(10.0));

        let mut schedule = Schedule::new();
        schedule.add_system("simulate", "read_step", read_step);

        tick(&mut world, &schedule, 0.1);
    }

    #[test]
    fn no_virtual_time_unchanged_behavior() {
        // Identical to existing tick_runs_correct_number_of_times
        let mut world = World::new();
        world.insert_resource(FixedTimestep::new(10.0));
        world.spawn((TickCounter(0),));

        let mut schedule = Schedule::new();
        schedule.add_system("simulate", "count", count_system);

        let ticks = tick(&mut world, &schedule, 0.25);
        assert_eq!(ticks, 2);
    }

    #[test]
    fn virtual_time_paused_zero_ticks() {
        let mut world = World::new();
        world.insert_resource(FixedTimestep::new(10.0));
        let mut vt = VirtualTime::new();
        vt.paused = true;
        world.insert_resource(vt);
        world.spawn((TickCounter(0),));

        let mut schedule = Schedule::new();
        schedule.add_system("simulate", "count", count_system);

        let ticks = tick(&mut world, &schedule, 1.0);
        assert_eq!(ticks, 0);

        let counts: Vec<u32> = world
            .query::<TickCounter>()
            .into_iter()
            .map(|(_, c)| c.0)
            .collect();
        assert_eq!(counts, vec![0]);
    }

    #[test]
    fn virtual_time_scale_doubles_ticks() {
        let mut world = World::new();
        world.insert_resource(FixedTimestep::new(10.0));
        let mut vt = VirtualTime::new();
        vt.scale = 2.0;
        world.insert_resource(vt);
        world.spawn((TickCounter(0),));

        let mut schedule = Schedule::new();
        schedule.add_system("simulate", "count", count_system);

        // 0.1s real at 2x scale = 0.2s virtual = 2 ticks at 10 Hz
        let ticks = tick(&mut world, &schedule, 0.1);
        assert_eq!(ticks, 2);
    }

    #[test]
    fn virtual_time_max_delta_prevents_death_spiral() {
        let mut world = World::new();
        world.insert_resource(FixedTimestep::new(10.0));
        world.insert_resource(VirtualTime::new()); // max_delta = 0.25
        world.spawn((TickCounter(0),));

        let mut schedule = Schedule::new();
        schedule.add_system("simulate", "count", count_system);

        // 5.0s real, clamped to 0.25s virtual = 2 ticks (not 50!)
        let ticks = tick(&mut world, &schedule, 5.0);
        assert_eq!(ticks, 2);
    }

    #[test]
    fn virtual_time_elapsed_accumulates() {
        let mut world = World::new();
        world.insert_resource(FixedTimestep::new(10.0));
        world.insert_resource(VirtualTime::new());

        let schedule = Schedule::new();

        tick(&mut world, &schedule, 0.1);
        tick(&mut world, &schedule, 0.15);

        let vt = world.resource::<VirtualTime>();
        assert!((vt.elapsed - 0.25).abs() < f64::EPSILON);
    }
}
