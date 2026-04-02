# Deadline Scheduler

The deadline scheduler fires events at specific points in time. Register
`(Timestamp, event)` pairs and the scheduler drains all overdue entries
each tick, writing them as `Events<T>` for game systems to read via
`EventReader<T>`.

## Concepts

- **`Timestamp`** — Microseconds since UNIX epoch. Lightweight, no external
  dependencies. Convert from `chrono`: `Timestamp::from_micros(dt.timestamp_micros())`.
- **`Clock`** — Injectable time source trait. `SystemClock` uses wall time;
  `TestClock` is manually controllable for deterministic tests.
- **`Deadlines<T>`** — Sorted resource storing pending deadline entries for
  event type `T`. Insert is O(log n), drain is O(k) where k = overdue count.
- **`DeadlineId`** — Opaque handle returned on schedule, used for cancellation.

## Setup

```rust
use galeon_engine::{Engine, Deadlines, Timestamp, TestClock, Clock};

// Define your event type.
struct TimedEvent { entity_id: u32 }

let mut engine = Engine::new();

// Register the deadline type (creates Deadlines<T> + Events<T>).
engine.world_mut().add_deadline_type::<TimedEvent>();
```

## Scheduling deadlines

```rust
// Schedule from setup code:
let id = engine.world_mut().schedule_deadline(
    Timestamp::from_secs(1700000000),
    TimedEvent { entity_id: 42 },
);

// Schedule from a system via Commands:
fn schedule_event(mut cmds: Commands<'_>) {
    cmds.schedule_deadline(
        Timestamp::from_secs(1700000060),
        TimedEvent { entity_id: 7 },
    );
}
```

## Cancellation

```rust
// Cancel from setup code:
engine.world_mut().cancel_deadline::<TimedEvent>(id);

// Cancel from a system via Commands:
fn abort_dispatch(mut cmds: Commands<'_>) {
    cmds.cancel_deadline::<TimedEvent>(saved_id);
}
```

## Automatic draining

`Schedule::run()` automatically drains all registered deadline types
every tick. The execution order is:

1. `drain_all_deadlines()` — reads the `Clock` resource, fires all
   overdue entries into `Events<T>` current buffer.
2. `update_events()` — swaps current → previous (fired deadlines now
   readable by `EventReader<T>`).
3. Systems run — `EventReader<T>` sees fired deadlines **this tick**.

Install a `Clock` resource for automatic draining to activate:

```rust
use galeon_engine::{SystemClock, Clock};

// Production: wall-clock time.
engine.world_mut().insert_resource(Box::new(SystemClock) as Box<dyn Clock>);

// Tests: controllable time.
use galeon_engine::TestClock;
engine.world_mut().insert_resource(
    Box::new(TestClock::new(Timestamp::from_secs(1000))) as Box<dyn Clock>,
);
```

If no `Clock` resource is present, deadline draining is silently skipped.

### Manual draining

For advanced use cases, you can drain a single type manually:

```rust
world.drain_deadlines::<TimedEvent>(Timestamp::now());
```

## Batch reconciliation

If the engine pauses or the server restarts, all overdue deadlines fire
in a single tick automatically. When `Schedule::run()` calls
`drain_all_deadlines()`, every entry where `now >= deadline` fires at
once — no special API needed.

## Testing with TestClock

```rust
use galeon_engine::{TestClock, Timestamp, Clock};

let mut clock = TestClock::new(Timestamp::from_secs(0));
world.insert_resource(Box::new(clock) as Box<dyn Clock>);

// Run schedule — nothing fires at t=0.
schedule.run(&mut world);

// Advance clock and run again — overdue deadlines fire.
world.resource_mut::<Box<dyn Clock>>()
    .downcast_mut::<TestClock>()
    .unwrap()
    .advance_secs(60);
schedule.run(&mut world);
```

## Integration with Events

Deadlines build on the existing Events API:
- `add_deadline_type::<T>()` calls `add_event::<T>()` internally and
  registers a drainer closure for automatic firing.
- `Schedule::run()` drains all deadline types before advancing event
  buffers, so fired events are readable in the same tick.
- Game systems read fired deadlines via `EventReader<T>` — same API as
  any other event.
