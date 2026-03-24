# ECS — Entity Component System

Galeon uses an ECS at its core. All game state lives here — entities are
lightweight IDs, components are plain data structs, and systems are functions
that operate on components.

## Entities

An entity is just an ID — a `u32` index paired with a `u32` generation. The
generation prevents bugs where you hold a stale reference to a despawned entity.

```rust
let entity = world.spawn((Position { x: 0.0, y: 0.0 },));
assert!(world.is_alive(entity));

world.despawn(entity);
assert!(!world.is_alive(entity));
```

## Components

A component is any Rust struct that derives `Component`. Components are pure
data — no behavior.

```rust
use galeon_engine::Component;

#[derive(Component)]
struct Position { x: f32, y: f32 }

#[derive(Component)]
struct Health { current: i32, max: i32 }

#[derive(Component)]
struct Velocity { x: f32, y: f32 }
```

## Spawning

Spawn entities with a tuple of components:

```rust
// Single component
let e = world.spawn((Position { x: 0.0, y: 0.0 },));

// Multiple components (up to 8)
let unit = world.spawn((
    Position { x: 10.0, y: 20.0 },
    Health { current: 100, max: 100 },
    Velocity { x: 0.0, y: 0.0 },
));
```

## Querying

Read components:

```rust
for (entity, pos) in world.query::<Position>() {
    println!("Entity at ({}, {})", pos.x, pos.y);
}
```

Modify components:

```rust
for (entity, pos) in world.query_mut::<Position>() {
    pos.x += 1.0;
}
```

Query two components at once:

```rust
for (entity, pos, vel) in world.query2::<Position, Velocity>() {
    println!("Moving entity at ({}, {}) with velocity ({}, {})", pos.x, pos.y, vel.x, vel.y);
}
```

> Queries return lazy iterators — call `.collect::<Vec<_>>()` if you need `len()` or indexing.

Query three components (immutable):

```rust
for (entity, pos, vel, hp) in world.query3::<Position, Velocity, Health>() {
    // Process entities with all three components
}
```

Query three components (mutable):

```rust
for (entity, pos, vel, hp) in world.query3_mut::<Position, Velocity, Health>() {
    pos.x += vel.x;
    hp.current -= 1;
}
```

Direct access by entity:

```rust
if let Some(health) = world.get::<Health>(unit) {
    println!("HP: {}/{}", health.current, health.max);
}
```

## Resources

Resources are world-global singletons — data that isn't tied to a specific
entity. Delta time, tick count, configuration.

```rust
struct DeltaTime(f64);

world.insert_resource(DeltaTime(0.016));

let dt = world.resource::<DeltaTime>().0;
```

## Systems

A system is a plain function that takes `&mut World`:

```rust
fn movement_system(world: &mut World) {
    let dt = world.resource::<DeltaTime>().0;
    for (_, pos, vel) in world.query2_mut::<Position, Velocity>() {
        pos.x += vel.x * dt;
        pos.y += vel.y * dt;
    }
}
```

## Schedule

Systems are grouped into stages. Stages run in the order they're registered.
Within a stage, systems run in registration order.

```rust
let mut schedule = Schedule::new();
schedule.add_system("input",    "read_input",  input_system);
schedule.add_system("simulate", "movement",    movement_system);
schedule.add_system("simulate", "combat",      combat_system);
schedule.add_system("sync",     "three_sync",  sync_system);

// Run one tick
schedule.run(&mut world);
```

The three-stage model (`input` → `simulate` → `sync`) ensures input is
processed before simulation, and simulation completes before rendering sync.

## Storage Internals

Components are stored in **typed sparse sets** — each component type gets its
own `Vec<T>` (no boxing, no `dyn Any`). This means:

- **Zero heap allocation per component** — data lives in a contiguous `Vec<T>`
- **Zero runtime downcasts on hot paths** — queries iterate typed data directly
- **O(1) insert/get/remove** — sparse set semantics
- **Dense iteration** — ideal for systems that touch many entities

The type erasure needed for the component registry happens once per query call
(at the storage level), not once per entity. This is a single `TypeId`
comparison — negligible compared to the old design which boxed every component
and downcast on every access.

### Hot vs Cold Storage

| Storage Class | Use For | Why |
|---------------|---------|-----|
| **Hot** (typed sparse set, default) | Transforms, movement, health, combat state, AI state | Iterated every tick, must be cache-friendly |
| **Cold** (future: separate store) | Names, debug tags, editor metadata | Rarely iterated, should not pollute hot storage |

Currently all components use typed sparse sets. Cold/sparse-side storage for
editor metadata is a planned future addition.
