# Engine, Builder API & Plugins

The `Engine` struct is the entry point for a Galeon game. It owns the [`World`]
(entities, components, resources) and the [`Schedule`] (systems), and exposes a
fluent builder API for wiring everything up.

## Creating an Engine

```rust
use galeon_engine::Engine;

let mut engine = Engine::new();
```

## Adding Systems

Systems are plain Rust functions `fn(&mut World)`. Register them with a stage
name and a display name:

```rust
use galeon_engine::{Engine, World};

fn move_units(world: &mut World) {
    // query and mutate components here
}

let mut engine = Engine::new();
engine.add_system("simulate", "move_units", move_units);
```

Calls are chainable:

```rust
engine
    .add_system("pre", "input", input_system)
    .add_system("simulate", "physics", physics_system)
    .add_system("post", "render_sync", render_sync_system);
```

Stages run in the order they are first registered. Systems within the same
stage run in registration order.

## Inserting Resources

Resources are world-global singletons (e.g. configuration, time, counters).

```rust
struct TickRate(f64);

engine.insert_resource(TickRate(10.0));
```

This is also chainable with `add_system` and `add_plugin`.

## Plugins

A `Plugin` bundles related systems and resources into a reusable unit.

```rust
use galeon_engine::{Engine, Plugin, World};

fn physics_system(_world: &mut World) {}
fn collision_system(_world: &mut World) {}

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, engine: &mut Engine) {
        engine
            .add_system("simulate", "physics", physics_system)
            .add_system("simulate", "collision", collision_system);
    }
}
```

Apply the plugin with `add_plugin`:

```rust
let mut engine = Engine::new();
engine.add_plugin(PhysicsPlugin);
```

Multiple plugins can be chained:

```rust
engine
    .add_plugin(PhysicsPlugin)
    .add_plugin(AudioPlugin)
    .add_plugin(NetworkPlugin);
```

## Running the Engine

### Fixed-Timestep Game Loop

`Engine::tick(elapsed)` advances the simulation by a fixed step. Pass the
wall-clock delta since the last frame. If no [`FixedTimestep`] resource has
been inserted, the default RTS rate (10 Hz) is used automatically.

```rust
// Somewhere in your platform loop:
let ticks = engine.tick(delta_seconds);
// `ticks` is the number of simulation steps executed this frame.
```

To use a custom tick rate, insert a [`FixedTimestep`] resource before the
first call:

```rust
use galeon_engine::FixedTimestep;

engine.insert_resource(FixedTimestep::new(30.0)); // 30 Hz
```

### Single-shot Execution

For tests or tools that don't need a game loop, `run_once()` runs the schedule
exactly once:

```rust
engine.run_once();
```

## Accessing World and Schedule

```rust
// Read entity count
let count = engine.world().entity_count();

// Spawn an entity
engine.world_mut().spawn((Position { x: 0.0, y: 0.0 },));

// Inspect registered systems
let num_systems = engine.schedule().system_count();
```

## Full Example

```rust
use galeon_engine::{Engine, Plugin, World};

// --- Components ---

#[derive(galeon_engine::Component)]
struct Position { x: f32, y: f32 }

// --- Systems ---

fn gravity(world: &mut World) {
    // apply gravity to all entities with Position
    for (_, pos) in world.query_mut::<Position>() {
        pos.y -= 9.8 * 0.1; // step = 0.1 s at 10 Hz
    }
}

// --- Plugin ---

pub struct GravityPlugin;

impl Plugin for GravityPlugin {
    fn build(&self, engine: &mut Engine) {
        engine.add_system("simulate", "gravity", gravity);
    }
}

// --- Entry point ---

fn main() {
    let mut engine = Engine::new();
    engine
        .add_plugin(GravityPlugin)
        .insert_resource(/* your config */());

    engine.world_mut().spawn((Position { x: 0.0, y: 100.0 },));

    // Fake game loop
    for _ in 0..10 {
        engine.tick(1.0 / 60.0); // 60 fps
    }
}
```
