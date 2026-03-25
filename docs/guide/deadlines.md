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
struct ShipArrival { ship_id: u32 }

let mut engine = Engine::new();

// Register the deadline type (creates Deadlines<T> + Events<T>).
engine.world_mut().add_deadline_type::<ShipArrival>();
```

## Scheduling deadlines

```rust
// Schedule from setup code:
let id = engine.world_mut().schedule_deadline(
    Timestamp::from_secs(1700000000),
    ShipArrival { ship_id: 42 },
);

// Schedule from a system via Commands:
fn dispatch_ship(mut cmds: Commands<'_>) {
    cmds.schedule_deadline(
        Timestamp::from_secs(1700000060),
        ShipArrival { ship_id: 7 },
    );
}
```

## Cancellation

```rust
// Cancel from setup code:
engine.world_mut().cancel_deadline::<ShipArrival>(id);

// Cancel from a system via Commands:
fn abort_dispatch(mut cmds: Commands<'_>) {
    cmds.cancel_deadline::<ShipArrival>(saved_id);
}
```

## Draining (firing) deadlines

Call `world.drain_deadlines::<T>(now)` to fire all entries where
`now >= deadline`. Fired events are written to `Events<T>` and become
readable by `EventReader<T>` on the next tick (after `update_events()`).

```rust
// In a system or tick loop:
fn drain_arrivals(world: &mut World) {
    let now = Timestamp::now(); // or from a Clock resource
    world.drain_deadlines::<ShipArrival>(now);
}
```

## Batch reconciliation

If the engine pauses or the server restarts, all overdue deadlines fire
in a single tick when `drain_deadlines` is called with the current time.
This catch-up behavior is automatic — no special API needed.

## Testing with TestClock

```rust
use galeon_engine::{TestClock, Timestamp};

let mut clock = TestClock::new(Timestamp::from_secs(0));

// Nothing fires at t=0.
world.drain_deadlines::<MyEvent>(clock.now());

// Advance time and drain again.
clock.advance_secs(60);
world.drain_deadlines::<MyEvent>(clock.now());
```

## Integration with Events

Deadlines build on the existing Events API:
- `add_deadline_type::<T>()` calls `add_event::<T>()` internally.
- `drain_deadlines::<T>(now)` calls `Events::<T>::send()` for each fired entry.
- Game systems read fired deadlines via `EventReader<T>` — same API as
  any other event.
