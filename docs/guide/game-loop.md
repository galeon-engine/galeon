# Game Loop

Galeon uses a fixed-step game loop. The simulation ticks at a constant rate
(default 10 Hz for RTS) regardless of the rendering frame rate. This ensures
deterministic behavior — the same inputs always produce the same outputs.

## Setup

The easiest way to configure the tick rate is through the `Engine` builder:

```rust
use galeon_engine::{Engine, Component, QueryMut};

#[derive(Component)]
struct Position { x: f32 }

fn movement_system(mut positions: QueryMut<'_, Position>) {
    for (_, pos) in positions.iter_mut() { pos.x += 1.0; }
}

let mut engine = Engine::new();
engine.set_tick_rate(30.0); // 30 Hz for action games (default: 10 Hz)
engine.add_system::<(QueryMut<'_, Position>,)>("simulate", "movement", movement_system);
```

Genre presets are also available: `FixedTimestep::default_rts()` (10 Hz),
`::strategy()` (20 Hz), `::action()` (30 Hz), `::fast()` (60 Hz).

For low-level control without the `Engine` wrapper:

```rust
use galeon_engine::{World, Schedule, FixedTimestep, QueryMut};
use galeon_engine::game_loop;

let mut world = World::new();
world.insert_resource(FixedTimestep::new(10.0)); // 10 ticks per second

let mut schedule = Schedule::new();
schedule.add_system::<(QueryMut<'_, Position>,)>("simulate", "movement", movement_system);
```

## Ticking

The host (Electrobun, browser, test harness) provides the clock. Each frame,
call `tick()` with the elapsed seconds since the last frame:

```rust
let ticks_run = game_loop::tick(&mut world, &mut schedule, elapsed_seconds);
```

If 0.25 seconds have elapsed at 10 Hz, the schedule runs twice (with 0.05s
remainder carried to the next frame).

## Reading the Timestep in Systems

Systems can read the `FixedTimestep` resource to get the step size:

```rust
fn movement_system(ts: Res<'_, FixedTimestep>) {
    let dt = ts.step;
    // dt = 0.1 at 10 Hz
}
```

## Why Fixed-Step?

- **Deterministic**: Same inputs → same outputs, regardless of frame rate
- **Multiplayer-safe**: Lockstep networking requires identical simulation
- **Stable physics**: No frame-rate-dependent behavior
